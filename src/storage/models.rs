use crate::domain::message::Message;
use crate::domain::user::User;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct UserRecord {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub created_at: Option<OffsetDateTime>,
}

impl From<UserRecord> for User {
    fn from(record: UserRecord) -> Self {
        Self {
            id: record.id,
            username: record.username,
            password_hash: record.password_hash,
            created_at: record.created_at,
        }
    }
}

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
