use serde::{Deserialize, Serialize};
use uuid::Uuid;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub recipient_id: Uuid,
    pub content: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
}
