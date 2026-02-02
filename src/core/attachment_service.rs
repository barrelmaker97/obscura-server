use crate::config::S3Config;
use crate::error::{AppError, Result};
use crate::storage::DbPool;
use crate::storage::attachment_repo::AttachmentRepository;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use axum::body::{Body, Bytes};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration as StdDuration;
use time::{Duration, OffsetDateTime};
use tokio::sync::mpsc;
use uuid::Uuid;

type AttachmentStreamReceiver =
    mpsc::Receiver<std::result::Result<Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>>;

// Wrapper to satisfy S3 SDK's Sync requirement for Body
struct SyncBody {
    rx: Arc<Mutex<AttachmentStreamReceiver>>,
}

impl http_body::Body for SyncBody {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let mut rx = self.rx.lock().unwrap();

        match rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(http_body::Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone)]
pub struct AttachmentService {
    pool: DbPool,
    repo: AttachmentRepository,
    s3_client: Client,
    config: S3Config,
    ttl_days: i64,
}

impl AttachmentService {
    pub fn new(pool: DbPool, repo: AttachmentRepository, s3_client: Client, config: S3Config, ttl_days: i64) -> Self {
        Self { pool, repo, s3_client, config, ttl_days }
    }

    pub async fn upload(&self, content_len: Option<usize>, body: Body) -> Result<(Uuid, i64)> {
        // 1. Check Content-Length (Early rejection)
        if let Some(len) = content_len
            && len > self.config.attachment_max_size_bytes
        {
            return Err(AppError::BadRequest("Attachment too large".into()));
        }

        let id = Uuid::new_v4();
        let key = id.to_string();

        // 2. Bridge Axum Body -> SyncBody with Size Limit enforcement
        let limit = self.config.attachment_max_size_bytes;
        let limited_body = Limited::new(body, limit);

        let (tx, rx) = mpsc::channel(2);
        let mut data_stream = limited_body.into_data_stream();

        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(item) = data_stream.next().await {
                match item {
                    Ok(bytes) => {
                        let frame_res = Ok(bytes);
                        if tx.send(frame_res).await.is_err() {
                            tracing::debug!(
                                "Attachment upload stream closed by receiver (S3 client likely finished or failed early)"
                            );
                            break;
                        }
                    }
                    Err(e) => {
                        let is_limit = e.downcast_ref::<LengthLimitError>().is_some();

                        let err_to_send: Box<dyn std::error::Error + Send + Sync> = if is_limit {
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Body too large"))
                        } else {
                            e
                        };

                        let _ = tx.send(Err(err_to_send)).await;
                        break;
                    }
                }
            }
        });

        let sync_body = SyncBody { rx: Arc::new(Mutex::new(rx)) };
        let byte_stream = ByteStream::from_body_1_x(sync_body);

        self.s3_client
            .put_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .set_content_length(content_len.map(|l| l as i64))
            .body(byte_stream)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("S3 Upload failed for key {}: {:?}", key, e);
                AppError::Internal
            })?;

        // 3. Record Metadata
        let expires_at = OffsetDateTime::now_utc() + Duration::days(self.ttl_days);
        self.repo.create(&self.pool, id, expires_at).await?;

        tracing::debug!("Attachment uploaded: {} (expires: {})", id, expires_at);

        Ok((id, expires_at.unix_timestamp()))
    }

    pub async fn download(&self, id: Uuid) -> Result<(u64, ByteStream)> {
        // 1. Check Existence & Expiry
        match self.repo.get_expires_at(&self.pool, id).await? {
            Some(expires_at) => {
                if expires_at < OffsetDateTime::now_utc() {
                    return Err(AppError::NotFound);
                }
            }
            None => return Err(AppError::NotFound),
        }

        // 2. Stream from S3
        let key = id.to_string();
        let output = self.s3_client.get_object().bucket(&self.config.bucket).key(&key).send().await.map_err(|e| {
            tracing::error!("S3 Download failed for {}: {:?}", key, e);
            AppError::NotFound
        })?;

        let content_length = output.content_length.unwrap_or(0);
        Ok((content_length as u64, output.body))
    }

    pub async fn run_cleanup_loop(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        // Run every hour
        let interval = StdDuration::from_secs(3600);
        let mut next_tick = tokio::time::Instant::now() + interval;

        while !*shutdown.borrow() {
            tokio::select! {
                _ = tokio::time::sleep_until(next_tick) => {
                    tracing::debug!("Running attachment cleanup...");

                    if let Err(e) = self.cleanup_batch().await {
                        tracing::error!("Attachment cleanup cycle failed: {:?}", e);
                    }
                    next_tick = tokio::time::Instant::now() + interval;
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Attachment cleanup loop shutting down...");
    }

    async fn cleanup_batch(&self) -> Result<()> {
        loop {
            // Fetch expired attachments (Limit 100 per cycle to avoid blocking)
            let ids = self.repo.fetch_expired(&self.pool, 100).await?;

            if ids.is_empty() {
                break;
            }

            tracing::info!("Found {} expired attachments to delete", ids.len());

            for id in ids {
                let key = id.to_string();

                // 1. Delete from S3
                let res = self.s3_client.delete_object().bucket(&self.config.bucket).key(&key).send().await;

                match res {
                    Ok(_) => {}
                    Err(aws_sdk_s3::error::SdkError::ServiceError(e)) => {
                        tracing::warn!("S3 delete error for {}: {:?}", key, e);
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("S3 network/transport error for {}: {:?}", key, e);
                        continue;
                    }
                }

                // 2. Delete from DB
                self.repo.delete(&self.pool, id).await?;
            }
        }
        Ok(())
    }
}
