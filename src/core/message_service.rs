use crate::storage::message_repo::MessageRepository;
use crate::error::Result;
use uuid::Uuid;

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
}
