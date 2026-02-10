use crate::domain::user::User;
use crate::error::Result;
use crate::storage::records::User as UserRecord;
use sqlx::PgConnection;

#[derive(Clone, Default)]
pub struct UserRepository {}

impl UserRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(level = "debug", skip(self, conn, password_hash))]
    pub async fn create(&self, conn: &mut PgConnection, username: &str, password_hash: &str) -> Result<User> {
        let user = sqlx::query_as::<_, UserRecord>(
            r#"
            INSERT INTO users (username, password_hash)
            VALUES ($1, $2)
            RETURNING id, username, password_hash, created_at
            "#,
        )
        .bind(username)
        .bind(password_hash)
        .fetch_one(conn)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e
                && db_err.code().as_deref() == Some("23505")
            {
                return crate::error::AppError::Conflict("Username already exists".into());
            }
            crate::error::AppError::Database(e)
        })?;

        Ok(user.into())
    }

    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub async fn find_by_username(&self, conn: &mut PgConnection, username: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, UserRecord>(
            r#"
            SELECT id, username, password_hash, created_at
            FROM users
            WHERE username = $1
            "#,
        )
        .bind(username)
        .fetch_optional(conn)
        .await?;

        Ok(user.map(Into::into))
    }
}
