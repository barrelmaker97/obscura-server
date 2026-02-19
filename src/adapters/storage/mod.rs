use crate::error::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;

pub mod s3;

pub use s3::S3Storage;

pub type StorageStream = BoxStream<'static, Result<Bytes>>;

#[async_trait]
pub trait ObjectStorage: Send + Sync + 'static {
    async fn put(&self, key: &str, stream: StorageStream, content_len: Option<usize>, max_size: usize) -> Result<()>;
    async fn get(&self, key: &str) -> Result<(u64, StorageStream)>;
    async fn head(&self, key: &str) -> Result<u64>;
    async fn delete(&self, key: &str) -> Result<()>;
}
