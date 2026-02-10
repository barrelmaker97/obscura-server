use crate::domain::user::User;
use crate::error::{AppError, Result};
use sqlx::PgConnection;

#[derive(Clone)]
pub struct IdentityService {
    repo: crate::storage::user_repo::UserRepository,
}

impl IdentityService {
    pub fn new(repo: crate::storage::user_repo::UserRepository) -> Self {
        Self { repo }
    }

    #[tracing::instrument(err, skip(self, conn, password_hash))]
    pub async fn create_user(&self, conn: &mut PgConnection, username: &str, password_hash: &str) -> Result<User> {
        self.repo.create(conn, username, password_hash).await.map_err(|e| {
            if let AppError::Database(sqlx::Error::Database(db_err)) = &e
                && db_err.code().as_deref() == Some("23505")
            {
                return AppError::Conflict("Username already exists".into());
            }
            e
        })
    }

    #[tracing::instrument(err, skip(self, conn))]
    pub async fn find_by_username(&self, conn: &mut PgConnection, username: &str) -> Result<Option<User>> {
        self.repo.find_by_username(conn, username).await
    }
}