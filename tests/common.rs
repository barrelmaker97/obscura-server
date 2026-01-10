use std::sync::Once;
use sqlx::PgPool;
use obscura_server::storage;

static INIT: Once = Once::new();

pub fn setup_tracing() {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "warn".into())
            .add_directive("obscura_server=debug".parse().unwrap())
            .add_directive("sqlx=warn".parse().unwrap())
            .add_directive("tower=warn".parse().unwrap())
            .add_directive("hyper=warn".parse().unwrap())
            .add_directive("reqwest=warn".parse().unwrap())
            .add_directive("rustls=warn".parse().unwrap())
            .add_directive("tungstenite=warn".parse().unwrap());

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
    });
}

pub async fn get_test_pool() -> PgPool {
    setup_tracing();
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());
    
    let pool = storage::init_pool(&database_url).await.expect("Failed to connect to DB. Is Postgres running?");
    
    // Run migrations automatically
    sqlx::migrate!().run(&pool).await.expect("Failed to run migrations");
    
    pool
}