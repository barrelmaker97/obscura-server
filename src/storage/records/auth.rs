use crate::domain::auth::RefreshToken;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct RefreshTokenRecord {
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

impl From<RefreshTokenRecord> for RefreshToken {
    fn from(record: RefreshTokenRecord) -> Self {
        Self {
            token_hash: record.token_hash,
            user_id: record.user_id,
            expires_at: record.expires_at,
            created_at: record.created_at,
        }
    }
}
