use crate::adapters::redis::RedisClient;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug)]
pub struct NotificationScheduler {
    redis: Arc<RedisClient>,
    queue_key: String,
}

impl NotificationScheduler {
    #[must_use]
    pub const fn new(redis: Arc<RedisClient>, queue_key: String) -> Self {
        Self { redis, queue_key }
    }

    /// Schedules a push notification for a user.
    /// Uses ZADD with the 'score' being the timestamp when the push should be delivered.
    /// Uses NX to ensure we don't overwrite an existing timer (coalescing multiple messages).
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    #[allow(clippy::cast_precision_loss)]
    pub async fn schedule_push(&self, user_id: Uuid, delay_secs: u64) -> anyhow::Result<()> {
        let run_at = time::OffsetDateTime::now_utc().unix_timestamp() + i64::try_from(delay_secs).unwrap_or(0);

        let mut conn = self.redis.publisher();
        // ZADD key NX score member
        let _: i64 = redis::cmd("ZADD")
            .arg(&self.queue_key)
            .arg("NX")
            .arg(run_at as f64)
            .arg(user_id.to_string())
            .query_async(&mut conn)
            .await?;
        Ok(())
    }

    /// Cancels a pending push notification for a user.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    pub async fn cancel_push(&self, user_id: Uuid) -> anyhow::Result<()> {
        let _ = self.redis.zrem(&self.queue_key, &user_id.to_string()).await?;
        Ok(())
    }

    /// Pulls a batch of due jobs from the queue using a "Read-then-Claim" pattern.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    #[allow(clippy::cast_precision_loss)]
    pub async fn pull_due_jobs(&self, limit: isize) -> anyhow::Result<Vec<Uuid>> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp() as f64;

        // 1. Read candidates
        let candidates = self.redis.zrange_byscore_limit(&self.queue_key, now, limit).await?;

        let mut claimed = Vec::new();

        // 2. Claim candidates one by one
        for member in candidates {
            // ZREM returns 1 if the item was removed (we claimed it), 0 if someone else did.
            match self.redis.zrem(&self.queue_key, &member).await {
                Ok(1) => {
                    if let Ok(id) = Uuid::parse_str(&member) {
                        claimed.push(id);
                    }
                }
                Ok(_) => {} // Someone else claimed it
                Err(e) => tracing::error!(error = %e, "Failed to claim job from Redis"),
            }
        }

        Ok(claimed)
    }
}
