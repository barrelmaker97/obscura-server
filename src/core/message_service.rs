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
        let count = self.repo.count_messages(recipient_id).await?;
        if count >= 1000 {
            self.repo.delete_oldest(recipient_id).await?;
        }

        self.repo.create(sender_id, recipient_id, content, 30).await?;
        Ok(())
    }

    /// Periodically cleans up expired messages from the database.
    pub async fn run_cleanup_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Every hour

        loop {
            interval.tick().await;
            tracing::debug!("Running expired message cleanup...");
            match self.repo.delete_expired().await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Cleanup: Deleted {} expired messages.", count);
                    }
                }
                Err(e) => {
                    tracing::error!("Cleanup loop error: {:?}", e);
                }
            }
        }
    }
}
