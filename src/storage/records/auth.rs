use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct RefreshToken {
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

impl From<RefreshToken> for crate::domain::auth::RefreshToken {
    fn from(record: RefreshToken) -> Self {
        Self {
            token_hash: record.token_hash,
            user_id: record.user_id,
            expires_at: record.expires_at,
            created_at: record.created_at,
        }
    }
}