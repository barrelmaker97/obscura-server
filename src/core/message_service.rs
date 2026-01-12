use crate::storage::message_repo::MessageRepository;
use crate::error::Result;
use uuid::Uuid;
use std::time::Duration;

#[derive(Clone)]
pub struct MessageService {
    repo: MessageRepository,
}

impl MessageService {
    pub fn new(repo: MessageRepository) -> Self {
        Self { repo }
    }

    pub async fn enqueue_message(&self, sender_id: Uuid, recipient_id: Uuid, content: Vec<u8>) -> Result<()> {
        // Optimization: We no longer check limits synchronously.
        // The background cleanup loop handles overflow.
        self.repo.create(sender_id, recipient_id, content, 30).await?;
        Ok(())
    }

    /// Periodically cleans up expired messages and enforces inbox limits.
    pub async fn run_cleanup_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes

        loop {
            interval.tick().await;
            tracing::debug!("Running message cleanup (expiry + limits)...");

            // 1. Delete Expired (TTL)
            match self.repo.delete_expired().await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Cleanup: Deleted {} expired messages.", count);
                    }
                }
                Err(e) => tracing::error!("Cleanup loop error (expiry): {:?}", e),
            }

            // 2. Enforce Inbox Limits (Global Overflow)
            // Limit to 1000 messages per user
            match self.repo.delete_global_overflow(1000).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Cleanup: Pruned {} overflow messages.", count);
                    }
                }
                Err(e) => tracing::error!("Cleanup loop error (overflow): {:?}", e),
            }
        }
    }
}
