use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct User {
    pub(crate) id: Uuid,
    #[allow(dead_code)]
    pub(crate) username: String,
    pub(crate) password_hash: String,
    #[allow(dead_code)]
    pub(crate) created_at: Option<OffsetDateTime>,
}
