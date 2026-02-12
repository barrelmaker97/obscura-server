use crate::domain::attachment::Attachment;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct AttachmentRecord {
    pub(crate) id: Uuid,
    pub(crate) expires_at: OffsetDateTime,
}

impl From<AttachmentRecord> for Attachment {
    fn from(record: AttachmentRecord) -> Self {
        Self { id: record.id, expires_at: record.expires_at }
    }
}
