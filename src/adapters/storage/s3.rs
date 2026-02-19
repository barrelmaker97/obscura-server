use crate::adapters::storage::{ObjectStorage, StorageError, StorageResult, StorageStream};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use futures::StreamExt;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
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

type SyncBodyReceiver = mpsc::Receiver<Result<Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>>;

struct SyncBody {
    rx: Arc<Mutex<SyncBodyReceiver>>,
}

impl http_body::Body for SyncBody {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
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
    ) -> StorageResult<()> {
        let (tx, rx) = mpsc::channel(2);
        let limit_exceeded = Arc::new(AtomicBool::new(false));

        let mut total_bytes = 0;
        let limit_signal = Arc::clone(&limit_exceeded);

        tokio::spawn(
            async move {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(bytes) => {
                            total_bytes += bytes.len();
                            if total_bytes > max_size {
                                // Signal that we hit the limit before closing the stream
                                limit_signal.store(true, Ordering::SeqCst);
                                let err: Box<dyn std::error::Error + Send + Sync> = Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "size limit exceeded",
                                ));
                                let _ = tx.send(Err(err)).await;
                                break;
                            }
                            if tx.send(Ok(bytes)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let err: Box<dyn std::error::Error + Send + Sync> = Box::new(e);
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

        let res = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .set_content_length(content_len.map(|l| i64::try_from(l).unwrap_or(i64::MAX)))
            .body(byte_stream)
            .send()
            .await;

        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                // Determine if failure was caused by our own internal size limit
                if limit_exceeded.load(Ordering::SeqCst) {
                    return Err(StorageError::ExceedsLimit);
                }

                tracing::error!(error = ?e, key = %key, "S3 Upload failed");
                Err(StorageError::Internal(e.to_string()))
            }
        }
    }

    async fn get(&self, key: &str) -> StorageResult<(u64, StorageStream)> {
        let output = self.client.get_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            if let aws_sdk_s3::error::SdkError::ServiceError(ref err) = e
                && err.err().is_no_such_key()
            {
                return StorageError::NotFound;
            }
            tracing::error!(error = ?e, key = %key, "S3 Download failed");
            StorageError::Internal(e.to_string())
        })?;

        let content_length = output.content_length.unwrap_or(0);

        let sdk_stream = output.body;
        let stream = futures::stream::unfold(sdk_stream, |mut s| async move {
            match s.next().await {
                Some(Ok(bytes)) => Some((Ok(bytes), s)),
                Some(Err(e)) => {
                    tracing::error!(error = ?e, "S3 Stream error");
                    Some((Err(std::io::Error::other(e.to_string())), s))
                }
                None => None,
            }
        })
        .boxed();

        Ok((u64::try_from(content_length).unwrap_or(0), stream))
    }

    async fn head(&self, key: &str) -> StorageResult<u64> {
        let output = self.client.head_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            if let aws_sdk_s3::error::SdkError::ServiceError(ref err) = e
                && err.err().is_not_found()
            {
                return StorageError::NotFound;
            }
            tracing::error!(error = ?e, key = %key, "S3 Head failed");
            StorageError::Internal(e.to_string())
        })?;
        let len = output.content_length.unwrap_or(0);
        Ok(u64::try_from(len).unwrap_or(0))
    }

    async fn delete(&self, key: &str) -> StorageResult<()> {
        self.client.delete_object().bucket(&self.bucket).key(key).send().await.map_err(|e| {
            tracing::error!(error = ?e, key = %key, "S3 Delete failed");
            StorageError::Internal(e.to_string())
        })?;
        Ok(())
    }
}
