#![allow(clippy::needless_raw_string_hashes)]
pub mod attachment_repo;
pub mod key_repo;
pub mod message_repo;
pub mod refresh_token_repo;
pub mod user_repo;
pub mod push_token_repo;
pub mod records;

use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

pub type DbPool = Pool<Postgres>;

/// Initializes the database connection pool.
///
/// # Errors
/// Returns `sqlx::Error` if the connection fails.
pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    PgPoolOptions::new().max_connections(20).connect(database_url).await
}
