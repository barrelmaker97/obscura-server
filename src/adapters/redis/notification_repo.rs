use crate::adapters::redis::{PubSubMessage, RedisClient};
use crate::domain::notification::UserEvent;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NotificationRepository {
    redis: Arc<RedisClient>,
    channel_prefix: String,
    push_queue_key: String,
}

impl NotificationRepository {
    #[must_use]
    pub const fn new(
        redis: Arc<RedisClient>,
        channel_prefix: String,
        push_queue_key: String,
    ) -> Self {
        Self {
            redis,
            channel_prefix,
            push_queue_key,
        }
    }

    /// Publishes a realtime event to a specific user.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    pub async fn publish_realtime(&self, user_id: Uuid, event: UserEvent) -> anyhow::Result<()> {
        let channel_name = format!("{}{user_id}", self.channel_prefix);
        let payload = [event as u8];
        let mut conn = self.redis.publisher();
        conn.publish::<_, _, i64>(&channel_name, &payload).await?;
        Ok(())
    }

    /// Subscribes to realtime events for all users.
    ///
    /// # Errors
    /// Returns an error if the subscription fails.
    pub async fn subscribe_realtime(&self) -> anyhow::Result<broadcast::Receiver<PubSubMessage>> {
        let pattern = format!("{}*", self.channel_prefix);
        self.redis.subscribe(&pattern).await
    }

    /// Returns the prefix used for user channels.
    #[must_use]
    pub fn channel_prefix(&self) -> &str {
        &self.channel_prefix
    }

    /// Schedules a push notification job for a user.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    #[allow(clippy::cast_precision_loss)]
    pub async fn push_job(&self, user_id: Uuid, delay_secs: u64) -> anyhow::Result<()> {
        let run_at = time::OffsetDateTime::now_utc().unix_timestamp() + i64::try_from(delay_secs).unwrap_or(0);

        let mut conn = self.redis.publisher();
        // ZADD key NX score member
        let _: i64 = redis::cmd("ZADD")
            .arg(&self.push_queue_key)
            .arg("NX")
            .arg(run_at as f64)
            .arg(user_id.to_string())
            .query_async(&mut conn)
            .await?;
        Ok(())
    }

    /// Cancels a pending push notification job for a user.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    pub async fn cancel_job(&self, user_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.redis.publisher();
        let _: i64 = conn.zrem(&self.push_queue_key, user_id.to_string()).await?;
        Ok(())
    }

    /// Pulls a batch of due push notification jobs from the queue.
    ///
    /// # Errors
    /// Returns an error if the Redis operation fails.
    #[allow(clippy::cast_precision_loss)]
    pub async fn claim_due_jobs(&self, limit: isize) -> anyhow::Result<Vec<Uuid>> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp() as f64;
        let mut conn = self.redis.publisher();

        // 1. Read candidates
        let candidates: Vec<String> = conn
            .zrangebyscore_limit(&self.push_queue_key, "-inf", now, 0, limit)
            .await?;

        let mut claimed = Vec::new();

        // 2. Claim candidates one by one
        for member in candidates {
            // zrem returns 1 if the item was removed (we claimed it), 0 if someone else did.
            match conn.zrem::<_, _, i64>(&self.push_queue_key, &member).await {
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
