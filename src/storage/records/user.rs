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
