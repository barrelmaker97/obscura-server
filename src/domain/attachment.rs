use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: Uuid,
    pub expires_at: OffsetDateTime,
}

impl Attachment {
    pub fn is_expired_at(&self, now: OffsetDateTime) -> bool {
        self.expires_at < now
    }
}
