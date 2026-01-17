use obscura_server::storage;
use sqlx::PgPool;
use std::sync::Once;

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

        tracing_subscriber::fmt().with_env_filter(filter).init();
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

#[allow(dead_code)]
pub fn get_test_config() -> obscura_server::config::Config {
    obscura_server::config::Config {
        database_url: "postgres://user:password@localhost/signal_server".to_string(),
        jwt_secret: "test_secret".to_string(),
        rate_limit_per_second: 10000,
        rate_limit_burst: 10000,
        auth_rate_limit_per_second: 10000,
        auth_rate_limit_burst: 10000,
        server_host: "127.0.0.1".to_string(),
        server_port: 0, // 0 means let OS choose
        message_ttl_days: 30,
        max_inbox_size: 1000,
        message_cleanup_interval_secs: 300,
        notification_gc_interval_secs: 60,
        notification_channel_capacity: 16,
        message_batch_limit: 50,
        trusted_proxies: "127.0.0.1/32,::1/128".to_string(),
        ws_outbound_buffer_size: 32,
        ws_ack_buffer_size: 100,
        ws_ack_batch_size: 50,
        ws_ack_flush_interval_ms: 500,
    }
}
