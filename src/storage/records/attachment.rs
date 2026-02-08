use crate::domain::attachment::Attachment;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct AttachmentRecord {
    pub id: Uuid,
    pub expires_at: OffsetDateTime,
}

impl From<AttachmentRecord> for Attachment {
    fn from(record: AttachmentRecord) -> Self {
        Self {
            id: record.id,
            expires_at: record.expires_at,
        }
    }
}
