use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct Attachment {
    pub id: Uuid,
    pub expires_at: OffsetDateTime,
}

impl From<Attachment> for crate::domain::attachment::Attachment {
    fn from(record: Attachment) -> Self {
        Self { id: record.id, expires_at: record.expires_at }
    }
}
