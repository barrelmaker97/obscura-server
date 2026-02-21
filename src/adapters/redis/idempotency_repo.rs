use crate::adapters::redis::RedisClient;
use redis::AsyncCommands;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct IdempotencyRepository {
    redis: Arc<RedisClient>,
    prefix: String,
}

impl IdempotencyRepository {
    #[must_use]
    pub fn new(redis: Arc<RedisClient>) -> Self {
        Self { redis, prefix: "idempotency:msg:".to_string() }
    }

    /// Retrieves a cached response for an idempotency key.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    pub async fn get_response(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let mut conn = self.redis.publisher();
        let full_key = format!("{}{key}", self.prefix);
        let response: Option<Vec<u8>> = conn.get(full_key).await?;
        Ok(response)
    }

    /// Saves a response for an idempotency key with a TTL.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    pub async fn save_response(&self, key: &str, response: &[u8], ttl_secs: u64) -> anyhow::Result<()> {
        let mut conn = self.redis.publisher();
        let full_key = format!("{}{key}", self.prefix);
        let _: () = conn.set_ex(full_key, response, ttl_secs).await?;
        Ok(())
    }
}
