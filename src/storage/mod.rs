use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

pub mod user_repo;
pub mod key_repo;
pub mod message_repo;

pub type DbPool = Pool<Postgres>;

pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
}