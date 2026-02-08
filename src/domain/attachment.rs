use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: Uuid,
    pub expires_at: OffsetDateTime,
}

impl Attachment {
    pub fn is_expired(&self) -> bool {
        self.expires_at < OffsetDateTime::now_utc()
    }
}
