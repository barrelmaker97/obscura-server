use std::env;
use dotenvy::dotenv;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub rate_limit_per_second: u32,
    pub rate_limit_burst: u32,
    pub server_host: String,
    pub server_port: u16,
    pub message_ttl_days: i64,
    pub max_inbox_size: i64,
    pub message_cleanup_interval_secs: u64,
    pub notification_gc_interval_secs: u64,
    pub notification_channel_capacity: usize,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        dotenv().ok();
        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            jwt_secret: env::var("JWT_SECRET")?,
            rate_limit_per_second: env::var("RATE_LIMIT_PER_SECOND")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
            rate_limit_burst: env::var("RATE_LIMIT_BURST")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .unwrap_or(10),
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .unwrap_or(3000),
            message_ttl_days: env::var("MESSAGE_TTL_DAYS")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .unwrap_or(30),
            max_inbox_size: env::var("MAX_INBOX_SIZE")
                .unwrap_or_else(|_| "1000".to_string())
                .parse()
                .unwrap_or(1000),
            message_cleanup_interval_secs: env::var("MESSAGE_CLEANUP_INTERVAL_SECS")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300),
            notification_gc_interval_secs: env::var("NOTIFICATION_GC_INTERVAL_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .unwrap_or(60),
            notification_channel_capacity: env::var("NOTIFICATION_CHANNEL_CAPACITY")
                .unwrap_or_else(|_| "16".to_string())
                .parse()
                .unwrap_or(16),
        })
    }
}
