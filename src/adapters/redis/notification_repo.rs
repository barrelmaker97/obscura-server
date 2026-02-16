use crate::adapters::redis::RedisClient;
use crate::config::NotificationConfig;
use crate::domain::notification::{RealtimeNotification, UserEvent};
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NotificationRepository {
    redis: Arc<RedisClient>,
    channel_prefix: String,
    push_queue_key: String,
    global_channel_capacity: usize,
}

impl NotificationRepository {
    #[must_use]
    pub fn new(
        redis: Arc<RedisClient>,
        config: &NotificationConfig,
    ) -> Self {
        Self {
            redis,
            channel_prefix: config.channel_prefix.clone(),
            push_queue_key: config.push_queue_key.clone(),
            global_channel_capacity: config.global_channel_capacity,
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
    pub async fn subscribe_realtime(&self) -> anyhow::Result<broadcast::Receiver<RealtimeNotification>> {
        let pattern = format!("{}*", self.channel_prefix);
        let mut redis_rx = self.redis.subscribe(&pattern).await?;
        
        let (tx, rx) = broadcast::channel(self.global_channel_capacity);
        let prefix = self.channel_prefix.clone();
        // Spawn a mapper task to translate technical PubSubMessages into domain RealtimeNotifications
        tokio::spawn(async move {
            while let Ok(msg) = redis_rx.recv().await {
                if let Some(user_id_str) = msg.channel.strip_prefix(&prefix)
                    && let Ok(user_id) = Uuid::parse_str(user_id_str)
                    && let Some(payload_byte) = msg.payload.first()
                    && let Ok(event) = UserEvent::try_from(*payload_byte)
                {
                    let _ = tx.send(RealtimeNotification { user_id, event });
                }
            }
        });

        Ok(rx)
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
