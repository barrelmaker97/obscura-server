use crate::core::user::User;
use crate::error::{AppError, Result};
use crate::storage::user_repo::UserRepository;
use sqlx::{Executor, Postgres};

#[derive(Clone)]
pub struct IdentityService {
    repo: UserRepository,
}

impl IdentityService {
    pub fn new(repo: UserRepository) -> Self {
        Self { repo }
    }

    pub async fn create_user<'e, E>(&self, executor: E, username: &str, password_hash: &str) -> Result<User>
    where
        E: Executor<'e, Database = Postgres>,
    {
        self.repo.create(executor, username, password_hash).await.map_err(|e| {
            if let AppError::Database(sqlx::Error::Database(db_err)) = &e
                && db_err.code().as_deref() == Some("23505")
            {
                return AppError::Conflict("Username already exists".into());
            }
            e
        })
    }

    pub async fn find_by_username<'e, E>(&self, executor: E, username: &str) -> Result<Option<User>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        self.repo.find_by_username(executor, username).await
    }
}
