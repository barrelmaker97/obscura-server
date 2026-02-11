use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct Message {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub recipient_id: Uuid,
    #[sqlx(rename = "message_type")]
    pub r#type: i32,
    pub content: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
}

impl From<Message> for crate::domain::message::Message {
    fn from(record: Message) -> Self {
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
