use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub(crate) id: Uuid,
    pub(crate) expires_at: OffsetDateTime,
}

impl Attachment {
    #[must_use]
    pub fn is_expired_at(&self, now: OffsetDateTime) -> bool {
        self.expires_at < now
    }
}
