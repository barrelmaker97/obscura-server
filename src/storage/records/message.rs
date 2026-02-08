use crate::domain::message::Message;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct MessageRecord {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub recipient_id: Uuid,
    pub message_type: i32,
    pub content: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
}

impl From<MessageRecord> for Message {
    fn from(record: MessageRecord) -> Self {
        Self {
            id: record.id,
            sender_id: record.sender_id,
            recipient_id: record.recipient_id,
            message_type: record.message_type,
            content: record.content,
            created_at: record.created_at,
            expires_at: record.expires_at,
        }
    }
}
