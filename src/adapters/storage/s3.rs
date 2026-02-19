use crate::adapters::storage::{ObjectStorage, StorageStream};
use crate::error::{AppError, Result};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use futures::StreamExt;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tracing::Instrument;

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

type SyncBodyReceiver = mpsc::Receiver<std::result::Result<Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>>;

// Wrapper to satisfy S3 SDK's Sync requirement for Body
struct SyncBody {
    rx: Arc<Mutex<SyncBodyReceiver>>,
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

#[async_trait]
impl ObjectStorage for S3Storage {
    async fn put(
        &self,
        key: &str,
        mut stream: StorageStream,
        content_len: Option<usize>,
        max_size: usize,
    ) -> Result<()> {
        let (tx, rx) = mpsc::channel(2);

        let mut total_bytes = 0;

        tokio::spawn(
            async move {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(bytes) => {
                            total_bytes += bytes.len();
                            if total_bytes > max_size {
                                let err: Box<dyn std::error::Error + Send + Sync> =
                                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Body too large"));
                                let _ = tx.send(Err(err)).await;
                                break;
                            }
                            if tx.send(Ok(bytes)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let err: Box<dyn std::error::Error + Send + Sync> =
                                Box::new(std::io::Error::other(e.to_string()));
                            let _ = tx.send(Err(err)).await;
                            break;
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("s3_upload_bridge")),
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

    async fn get(&self, key: &str) -> Result<(u64, StorageStream)> {
        let output = self.client.get_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            tracing::error!(error = ?e, key = %key, "S3 Download failed");
            AppError::NotFound
        })?;

        let content_length = output.content_length.unwrap_or(0);

        // Convert ByteStream to our neutral StorageStream
        // We use from_stream to ensure it implements Stream correctly
        let sdk_stream = output.body;
        let stream = futures::stream::unfold(sdk_stream, |mut s| async move {
            match s.next().await {
                Some(Ok(bytes)) => Some((Ok(bytes), s)),
                Some(Err(e)) => {
                    tracing::error!(error = ?e, "S3 Stream error");
                    Some((Err(AppError::Internal), s))
                }
                None => None,
            }
        })
        .boxed();

        Ok((u64::try_from(content_length).unwrap_or(0), stream))
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
