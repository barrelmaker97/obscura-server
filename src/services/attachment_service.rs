use crate::config::StorageConfig;
use crate::error::{AppError, Result};
use crate::storage::DbPool;
use crate::storage::attachment_repo::AttachmentRepository;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use axum::body::{Body, Bytes};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use opentelemetry::{global, metrics::{Counter, Histogram}};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration as StdDuration;
use time::{Duration, OffsetDateTime};
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone)]
struct Metrics {
    uploaded_bytes: Counter<u64>,
    upload_size_bytes: Histogram<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            uploaded_bytes: meter
                .u64_counter("attachments_uploaded_bytes")
                .with_description("Total bytes of attachments uploaded")
                .build(),
            upload_size_bytes: meter
                .u64_histogram("attachments_upload_size_bytes")
                .with_description("Distribution of attachment upload sizes")
                .build(),
        }
    }
}

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
    config: StorageConfig,
    ttl_days: i64,
    metrics: Metrics,
}

impl AttachmentService {
    pub fn new(pool: DbPool, repo: AttachmentRepository, s3_client: Client, config: StorageConfig, ttl_days: i64) -> Self {
        Self {
            pool,
            repo,
            s3_client,
            config,
            ttl_days,
            metrics: Metrics::new(),
        }
    }

    #[tracing::instrument(
        err(level = "warn"),
        skip(self, body),
        fields(attachment_id = tracing::field::Empty, attachment_size = tracing::field::Empty)
    )]
    pub async fn upload(&self, content_len: Option<usize>, body: Body) -> Result<(Uuid, i64)> {
        if let Some(len) = content_len {
            tracing::Span::current().record("attachment.size", len);
            if len > self.config.attachment_max_size_bytes {
                return Err(AppError::BadRequest("Attachment too large".into()));
            }
        }

        let id = Uuid::new_v4();
        let key = id.to_string();
        tracing::Span::current().record("attachment.id", tracing::field::display(id));

        // Bridge Axum Body -> SyncBody with size limit enforcement to satisfy S3 SDK's requirements
        let limit = self.config.attachment_max_size_bytes;
        let limited_body = Limited::new(body, limit);

        let (tx, rx) = mpsc::channel(2);
        let mut data_stream = limited_body.into_data_stream();

        use tracing::Instrument;
        tokio::spawn(
            async move {
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
            }
            .instrument(tracing::info_span!("attachment_stream_bridge")),
        );

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
                tracing::error!(error = ?e, key = %key, "S3 Upload failed");
                AppError::Internal
            })?;

        let expires_at = OffsetDateTime::now_utc() + Duration::days(self.ttl_days);
        let mut conn = self.pool.acquire().await?;
        self.repo.create(&mut conn, id, expires_at).await?;

        tracing::debug!(attachment_id = %id, expires_at = %expires_at, "Attachment uploaded");

        if let Some(len) = content_len {
            self.metrics.uploaded_bytes.add(len as u64, &[]);
            self.metrics.upload_size_bytes.record(len as u64, &[]);
        }

        Ok((id, expires_at.unix_timestamp()))
    }

    #[tracing::instrument(
        err(level = "warn"),
        skip(self),
        fields(attachment_id = %id, attachment_size = tracing::field::Empty)
    )]
    pub async fn download(&self, id: Uuid) -> Result<(u64, ByteStream)> {
        // 1. Check Existence & Expiry using Domain Logic
        let mut conn = self.pool.acquire().await?;
        match self.repo.find_by_id(&mut conn, id).await? {
            Some(attachment) => {
                if attachment.is_expired_at(OffsetDateTime::now_utc()) {
                    return Err(AppError::NotFound);
                }
            }
            None => return Err(AppError::NotFound),
        }

        // 2. Stream from S3
        let key = id.to_string();
        let output = self.s3_client.get_object().bucket(&self.config.bucket).key(&key).send().await.map_err(|e| {
            tracing::error!(error = ?e, key = %key, "S3 Download failed");
            AppError::NotFound
        })?;

        let content_length = output.content_length.unwrap_or(0);
        tracing::Span::current().record("attachment.size", content_length);

        tracing::debug!("Attachment download successful");
        Ok((content_length as u64, output.body))
    }

    pub async fn run_cleanup_loop(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let interval = StdDuration::from_secs(self.config.cleanup_interval_secs);
        let mut next_tick = tokio::time::Instant::now() + interval;

        while !*shutdown.borrow() {
            tokio::select! {
                () = tokio::time::sleep_until(next_tick) => {
                    async {
                        tracing::debug!("Running attachment cleanup...");

                        if let Err(e) = self.cleanup_batch().await {
                            tracing::error!(error = %e, "Attachment cleanup cycle failed");
                        }
                    }
                    .instrument(tracing::info_span!("attachment_cleanup_iteration"))
                    .await;
                    next_tick = tokio::time::Instant::now() + interval;
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Attachment cleanup loop shutting down...");
    }

    #[tracing::instrument(
        err,
        skip(self),
        fields(batch_count = tracing::field::Empty)
    )]
    async fn cleanup_batch(&self) -> Result<()> {
        loop {
            // Fetch expired attachments
            let mut conn = self.pool.acquire().await?;
            let ids = self.repo.fetch_expired(&mut conn, self.config.cleanup_batch_size as i64).await?;

            if ids.is_empty() {
                break;
            }

            tracing::Span::current().record("batch.count", ids.len());
            tracing::info!(count = %ids.len(), "Found expired attachments to delete");

            let count = ids.len();
            for id in ids {
                let key = id.to_string();
                let res = async {
                    // Delete object from S3 first to avoid orphaned files
                    let res = self.s3_client
                        .delete_object()
                        .bucket(&self.config.bucket)
                        .key(&key)
                        .send()
                        .await;

                    match res {
                        Ok(_) => {}
                        Err(aws_sdk_s3::error::SdkError::ServiceError(e)) => {
                            tracing::warn!(error = ?e, key = %key, "S3 delete error");
                            return Ok(()); // Skip DB delete if S3 failed to ensure we don't lose the reference
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, key = %key, "S3 network/transport error");
                            return Ok(());
                        }
                    }

                    // Only delete from DB if S3 deletion was successful
                    let mut conn = self.pool.acquire().await?;
                    self.repo.delete(&mut conn, id).await
                }
                .instrument(tracing::info_span!("delete_attachment", "attachment.id" = %id))
                .await;

                res?;
            }
            tracing::info!(deleted_count = %count, "Attachment cleanup batch completed successfully");
        }
        Ok(())
    }
}
