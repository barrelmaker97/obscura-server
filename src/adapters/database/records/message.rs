use crate::domain::message::Message;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct MessageRecord {
    pub(crate) id: Uuid,
    pub(crate) sender_id: Uuid,
    pub(crate) recipient_id: Uuid,
    #[sqlx(rename = "message_type")]
    pub(crate) r#type: i32,
    pub(crate) content: Vec<u8>,
    pub(crate) created_at: Option<OffsetDateTime>,
    pub(crate) expires_at: OffsetDateTime,
}

impl From<MessageRecord> for Message {
    fn from(record: MessageRecord) -> Self {
        Self {
            id: record.id,
            sender_id: record.sender_id,
            recipient_id: record.recipient_id,
            message_type: record.r#type,
            content: record.content,
            created_at: record.created_at,
            expires_at: record.expires_at,
        }
    }
}
