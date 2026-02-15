use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Message {
    pub(crate) id: Uuid,
    pub(crate) sender_id: Uuid,
    #[allow(dead_code)]
    pub(crate) recipient_id: Uuid,
    pub(crate) r#type: i32,
    pub(crate) content: Vec<u8>,
    pub(crate) created_at: Option<OffsetDateTime>,
    pub(crate) expires_at: OffsetDateTime,
}

impl Message {
    #[must_use]
    pub fn is_expired_at(&self, now: OffsetDateTime) -> bool {
        self.expires_at < now
    }
}
