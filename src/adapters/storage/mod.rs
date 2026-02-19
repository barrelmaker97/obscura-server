use crate::error::Result;
use async_trait::async_trait;
use aws_sdk_s3::primitives::ByteStream;
use axum::body::Body;

pub mod s3;

pub use s3::S3Storage;

#[async_trait]
pub trait ObjectStorage: Send + Sync + 'static {
    async fn put(&self, key: &str, body: Body, content_len: Option<usize>, max_size: usize) -> Result<()>;
    async fn get(&self, key: &str) -> Result<(u64, ByteStream)>;
    async fn head(&self, key: &str) -> Result<u64>;
    async fn delete(&self, key: &str) -> Result<()>;
}
