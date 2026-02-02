use crate::core::user::User;
use crate::error::Result;
use sqlx::{Executor, Postgres};

#[derive(Clone, Default)]
pub struct UserRepository {}

impl UserRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(level = "debug", skip(self, executor, password_hash))]
    pub async fn create<'e, E>(&self, executor: E, username: &str, password_hash: &str) -> Result<User>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (username, password_hash)
            VALUES ($1, $2)
            RETURNING id, username, password_hash, created_at
            "#,
        )
        .bind(username)
        .bind(password_hash)
        .fetch_one(executor)
        .await?;

        Ok(user)
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn find_by_username<'e, E>(&self, executor: E, username: &str) -> Result<Option<User>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let user = sqlx::query_as::<_, User>(
            r#"
            SELECT id, username, password_hash, created_at
            FROM users
            WHERE username = $1
            "#,
        )
        .bind(username)
        .fetch_optional(executor)
        .await?;

        Ok(user)
    }
}
