use crate::adapters::database::DbPool;
use crate::adapters::database::attachment_repo::AttachmentRepository;
use crate::config::StorageConfig;
use crate::error::{AppError, Result};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use axum::body::{Body, Bytes};
use futures::StreamExt;
use http_body_util::{BodyExt, LengthLimitError, Limited};
use opentelemetry::{
    global,
    metrics::{Counter, Histogram},
};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use time::{Duration, OffsetDateTime};
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) struct Metrics {
    pub(crate) uploaded_bytes: Counter<u64>,
    pub(crate) upload_size_bytes: Histogram<u64>,
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
        let mut rx = self.rx.lock().expect("Failed to lock receiver mutex");

        match rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(http_body::Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AttachmentService {
    pool: DbPool,
    repo: AttachmentRepository,
    s3_client: Client,
    config: StorageConfig,
    ttl_days: i64,
    metrics: Metrics,
}

impl AttachmentService {
    #[must_use]
    pub fn new(
        pool: DbPool,
        repo: AttachmentRepository,
        s3_client: Client,
        config: StorageConfig,
        ttl_days: i64,
    ) -> Self {
        Self { pool, repo, s3_client, config, ttl_days, metrics: Metrics::new() }
    }

    /// Uploads an attachment to S3 and records it in the database.
    ///
    /// # Errors
    /// Returns `AppError::BadRequest` if the attachment exceeds the size limit.
    /// Returns `AppError::Internal` if S3 or the database fails.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self, body),
        fields(attachment_id = tracing::field::Empty, attachment_size = tracing::field::Empty)
    )]
    pub(crate) async fn upload(&self, content_len: Option<usize>, body: Body) -> Result<(Uuid, i64)> {
        if let Some(len) = content_len {
            tracing::Span::current().record("attachment_size", len);
            if len > self.config.attachment_max_size_bytes {
                return Err(AppError::BadRequest("Attachment too large".into()));
            }
        }

        let id = Uuid::new_v4();
        let key = id.to_string();
        tracing::Span::current().record("attachment_id", tracing::field::display(id));

        // Bridge Axum Body -> SyncBody with size limit enforcement to satisfy S3 SDK's requirements
        let limit = self.config.attachment_max_size_bytes;
        let limited_body = Limited::new(body, limit);

        let (tx, rx) = mpsc::channel(2);
        let mut data_stream = limited_body.into_data_stream();

        tokio::spawn(
            async move {
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
            .set_content_length(content_len.map(|l| i64::try_from(l).unwrap_or(i64::MAX)))
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

    /// Downloads an attachment from S3.
    ///
    /// # Errors
    /// Returns `AppError::NotFound` if the attachment does not exist or has expired.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self),
        fields(attachment_id = %id, attachment_size = tracing::field::Empty)
    )]
    pub(crate) async fn download(&self, id: Uuid) -> Result<(u64, ByteStream)> {
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
        tracing::Span::current().record("attachment_size", content_length);

        tracing::debug!("Attachment download successful");
        Ok((u64::try_from(content_length).unwrap_or(0), output.body))
    }
}
