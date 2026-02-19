use crate::error::{AppError, Result};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use axum::body::Body;
use futures::StreamExt;
use http_body_util::{BodyExt, LengthLimitError, Limited};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tracing::Instrument;

#[async_trait]
pub trait ObjectStorage: Send + Sync + 'static {
    async fn put(&self, key: &str, body: Body, content_len: Option<usize>, max_size: usize) -> Result<()>;
    async fn get(&self, key: &str) -> Result<(u64, ByteStream)>;
    async fn head(&self, key: &str) -> Result<u64>;
    async fn delete(&self, key: &str) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct S3Storage {
    client: Client,
    bucket: String,
}

impl S3Storage {
    #[must_use]
    pub const fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }
}

type SyncBodyReceiver = mpsc::Receiver<std::result::Result<axum::body::Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>>;

// Wrapper to satisfy S3 SDK's Sync requirement for Body
struct SyncBody {
    rx: Arc<Mutex<SyncBodyReceiver>>,
}

impl http_body::Body for SyncBody {
    type Data = axum::body::Bytes;
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

#[async_trait]
impl ObjectStorage for S3Storage {
    async fn put(&self, key: &str, body: Body, content_len: Option<usize>, max_size: usize) -> Result<()> {
        let limited_body = Limited::new(body, max_size);
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
                                    "S3 upload stream closed by receiver (likely finished or failed early)"
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
            .instrument(tracing::info_span!("s3_stream_bridge")),
        );

        let sync_body = SyncBody { rx: Arc::new(Mutex::new(rx)) };
        let byte_stream = ByteStream::from_body_1_x(sync_body);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .set_content_length(content_len.map(|l| i64::try_from(l).unwrap_or(i64::MAX)))
            .body(byte_stream)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error = ?e, key = %key, "S3 Upload failed");
                AppError::Internal
            })?;

        Ok(())
    }

    async fn get(&self, key: &str) -> Result<(u64, ByteStream)> {
        let output = self.client.get_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            tracing::error!(error = ?e, key = %key, "S3 Download failed");
            AppError::NotFound
        })?;

        let content_length = output.content_length.unwrap_or(0);
        Ok((u64::try_from(content_length).unwrap_or(0), output.body))
    }

    async fn head(&self, key: &str) -> Result<u64> {
        let output = self.client.head_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            tracing::error!(error = ?e, key = %key, "S3 Head failed");
            AppError::NotFound
        })?;
        let len = output.content_length.unwrap_or(0);
        Ok(u64::try_from(len).unwrap_or(0))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.client.delete_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            tracing::error!(error = ?e, key = %key, "S3 Delete failed");
            AppError::Internal
        })?;
        Ok(())
    }
}
