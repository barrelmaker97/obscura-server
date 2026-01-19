use clap::Parser;

const DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 10;
const DEFAULT_RATE_LIMIT_BURST: u32 = 20;
const DEFAULT_AUTH_RATE_LIMIT_PER_SECOND: u32 = 1;
const DEFAULT_AUTH_RATE_LIMIT_BURST: u32 = 3;
const DEFAULT_SERVER_HOST: &str = "0.0.0.0";
const DEFAULT_SERVER_PORT: u16 = 3000;
const DEFAULT_MESSAGE_TTL_DAYS: i64 = 30;
const DEFAULT_MAX_INBOX_SIZE: i64 = 1000;
const DEFAULT_MESSAGE_CLEANUP_INTERVAL_SECS: u64 = 300;
const DEFAULT_NOTIFICATION_GC_INTERVAL_SECS: u64 = 60;
const DEFAULT_NOTIFICATION_CHANNEL_CAPACITY: usize = 16;
const DEFAULT_MESSAGE_BATCH_LIMIT: i64 = 50;
const DEFAULT_TRUSTED_PROXIES: &str = "10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,127.0.0.1/32";
const DEFAULT_WS_OUTBOUND_BUFFER_SIZE: usize = 32;
const DEFAULT_WS_ACK_BUFFER_SIZE: usize = 100;
const DEFAULT_WS_ACK_BATCH_SIZE: usize = 50;
const DEFAULT_WS_ACK_FLUSH_INTERVAL_MS: u64 = 500;
const DEFAULT_ACCESS_TOKEN_TTL_SECS: u64 = 900; // 15 minutes
const DEFAULT_REFRESH_TOKEN_TTL_DAYS: i64 = 30; // 30 days

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    #[arg(long, env)]
    pub database_url: String,

    #[arg(long, env)]
    pub jwt_secret: String,

    #[arg(long, env, default_value_t = DEFAULT_ACCESS_TOKEN_TTL_SECS)]
    pub access_token_ttl_secs: u64,

    #[arg(long, env, default_value_t = DEFAULT_REFRESH_TOKEN_TTL_DAYS)]
    pub refresh_token_ttl_days: i64,

    #[arg(long, env, default_value_t = DEFAULT_RATE_LIMIT_PER_SECOND)]
    pub rate_limit_per_second: u32,

    #[arg(long, env, default_value_t = DEFAULT_RATE_LIMIT_BURST)]
    pub rate_limit_burst: u32,

    /// Stricter rate limit for expensive auth-related endpoints (register/login)
    #[arg(long, env, default_value_t = DEFAULT_AUTH_RATE_LIMIT_PER_SECOND)]
    pub auth_rate_limit_per_second: u32,

    /// Burst allowance for expensive auth-related endpoints
    #[arg(long, env, default_value_t = DEFAULT_AUTH_RATE_LIMIT_BURST)]
    pub auth_rate_limit_burst: u32,

    #[arg(long, env, default_value = DEFAULT_SERVER_HOST)]
    pub server_host: String,

    #[arg(long, env = "PORT", default_value_t = DEFAULT_SERVER_PORT)]
    pub server_port: u16,

    #[arg(long, env, default_value_t = DEFAULT_MESSAGE_TTL_DAYS)]
    pub message_ttl_days: i64,

    #[arg(long, env, default_value_t = DEFAULT_MAX_INBOX_SIZE)]
    pub max_inbox_size: i64,

    #[arg(long, env, default_value_t = DEFAULT_MESSAGE_CLEANUP_INTERVAL_SECS)]
    pub message_cleanup_interval_secs: u64,

    #[arg(long, env, default_value_t = DEFAULT_NOTIFICATION_GC_INTERVAL_SECS)]
    pub notification_gc_interval_secs: u64,

    #[arg(long, env, default_value_t = DEFAULT_NOTIFICATION_CHANNEL_CAPACITY)]
    pub notification_channel_capacity: usize,

    #[arg(long, env, default_value_t = DEFAULT_MESSAGE_BATCH_LIMIT)]
    pub message_batch_limit: i64,

    /// Comma-separated list of CIDRs to trust for X-Forwarded-For IP extraction
    #[arg(long, env, default_value = DEFAULT_TRUSTED_PROXIES)]
    pub trusted_proxies: String,

    #[arg(long, env, default_value_t = DEFAULT_WS_OUTBOUND_BUFFER_SIZE)]
    pub ws_outbound_buffer_size: usize,

    #[arg(long, env, default_value_t = DEFAULT_WS_ACK_BUFFER_SIZE)]
    pub ws_ack_buffer_size: usize,

    #[arg(long, env, default_value_t = DEFAULT_WS_ACK_BATCH_SIZE)]
    pub ws_ack_batch_size: usize,

    #[arg(long, env, default_value_t = DEFAULT_WS_ACK_FLUSH_INTERVAL_MS)]
    pub ws_ack_flush_interval_ms: u64,

    // --- S3 / MinIO Configuration ---
    #[arg(long, env)]
    pub s3_bucket: String,

    #[arg(long, env, default_value = "us-east-1")]
    pub s3_region: String,

    #[arg(long, env)]
    pub s3_endpoint: Option<String>,

    #[arg(long, env)]
    pub s3_access_key: Option<String>,

    #[arg(long, env)]
    pub s3_secret_key: Option<String>,

    /// Force path style (required for many MinIO setups: http://host/bucket/key)
    #[arg(long, env, default_value_t = false)]
    pub s3_force_path_style: bool,

    #[arg(long, env, default_value_t = 30)]
    pub attachment_ttl_days: i64,

    /// Max attachment size in bytes (Default: 50MB)
    #[arg(long, env, default_value_t = 52_428_800)]
    pub attachment_max_size_bytes: usize,
}

impl Config {
    pub fn load() -> Self {
        Self::parse()
    }
}
