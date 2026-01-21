use clap::{Args, Parser};
use ipnetwork::IpNetwork;

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Database connection URL
    #[arg(long, env = "OBSCURA_DATABASE_URL")]
    pub database_url: String,

    /// Global time-to-live for messages and attachments in days
    #[arg(long, env = "OBSCURA_TTL_DAYS", default_value_t = 30)]
    pub ttl_days: i64,

    #[command(flatten)]
    pub server: ServerConfig,

    #[command(flatten)]
    pub auth: AuthConfig,

    #[command(flatten)]
    pub rate_limit: RateLimitConfig,

    #[command(flatten)]
    pub messaging: MessagingConfig,

    #[command(flatten)]
    pub notifications: NotificationConfig,

    #[command(flatten)]
    pub websocket: WsConfig,

    #[command(flatten)]
    pub s3: S3Config,
}

#[derive(Clone, Debug, Args)]
pub struct ServerConfig {
    /// Host to listen on
    #[arg(long, env = "OBSCURA_HOST", default_value = "0.0.0.0")]
    pub host: String,

    /// Port to listen on
    #[arg(long, env = "OBSCURA_PORT", default_value_t = 3000)]
    pub port: u16,

    /// Comma-separated list of CIDRs to trust for X-Forwarded-For IP extraction
    #[arg(
        long,
        env = "OBSCURA_TRUSTED_PROXIES",
        default_value = "10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,127.0.0.1/32",
        value_delimiter = ','
    )]
    pub trusted_proxies: Vec<IpNetwork>,
}

#[derive(Clone, Debug, Args)]
pub struct AuthConfig {
    /// Secret key for JWT signing
    #[arg(long, env = "OBSCURA_JWT_SECRET")]
    pub jwt_secret: String,

    /// Access token time-to-live in seconds
    #[arg(long, env = "OBSCURA_ACCESS_TOKEN_TTL_SECS", default_value_t = 900)]
    pub access_token_ttl_secs: u64,

    /// Refresh token time-to-live in days
    #[arg(long, env = "OBSCURA_REFRESH_TOKEN_TTL_DAYS", default_value_t = 30)]
    pub refresh_token_ttl_days: i64,
}

#[derive(Clone, Debug, Args)]
pub struct RateLimitConfig {
    /// Requests per second allowed for standard endpoints
    #[arg(long, env = "OBSCURA_RATE_LIMIT_PER_SECOND", default_value_t = 10)]
    pub per_second: u32,

    /// Burst allowance for standard endpoints
    #[arg(long, env = "OBSCURA_RATE_LIMIT_BURST", default_value_t = 20)]
    pub burst: u32,

    /// Stricter rate limit for expensive auth-related endpoints (register/login)
    #[arg(long, env = "OBSCURA_AUTH_RATE_LIMIT_PER_SECOND", default_value_t = 1)]
    pub auth_per_second: u32,

    /// Burst allowance for expensive auth-related endpoints
    #[arg(long, env = "OBSCURA_AUTH_RATE_LIMIT_BURST", default_value_t = 3)]
    pub auth_burst: u32,
}

#[derive(Clone, Debug, Args)]
pub struct MessagingConfig {
    /// Maximum number of messages in a user's inbox
    #[arg(long, env = "OBSCURA_MAX_INBOX_SIZE", default_value_t = 1000)]
    pub max_inbox_size: i64,

    /// How often to run the message cleanup task
    #[arg(long, env = "OBSCURA_CLEANUP_INTERVAL_SECS", default_value_t = 300)]
    pub cleanup_interval_secs: u64,

    /// Maximum number of messages to process in a single batch
    #[arg(long, env = "OBSCURA_BATCH_LIMIT", default_value_t = 50)]
    pub batch_limit: i64,

    /// Threshold of one-time prekeys to trigger a refill notification
    #[arg(long, env = "OBSCURA_PRE_KEY_REFILL_THRESHOLD", default_value_t = 20)]
    pub pre_key_refill_threshold: i32,

    /// Maximum number of one-time prekeys allowed per user
    #[arg(long, env = "OBSCURA_MAX_PRE_KEYS", default_value_t = 100)]
    pub max_pre_keys: i64,
}

#[derive(Clone, Debug, Args)]
pub struct NotificationConfig {
    /// How often to run the notification garbage collection
    #[arg(long, env = "OBSCURA_GC_INTERVAL_SECS", default_value_t = 60)]
    pub gc_interval_secs: u64,

    /// Capacity of the notification channel
    #[arg(long, env = "OBSCURA_CHANNEL_CAPACITY", default_value_t = 16)]
    pub channel_capacity: usize,
}

#[derive(Clone, Debug, Args)]
pub struct WsConfig {
    /// Size of the outbound message buffer
    #[arg(long, env = "OBSCURA_WS_OUTBOUND_BUFFER_SIZE", default_value_t = 32)]
    pub outbound_buffer_size: usize,

    /// Size of the acknowledgment buffer
    #[arg(long, env = "OBSCURA_WS_ACK_BUFFER_SIZE", default_value_t = 100)]
    pub ack_buffer_size: usize,

    /// Number of acknowledgments to batch before flushing
    #[arg(long, env = "OBSCURA_WS_ACK_BATCH_SIZE", default_value_t = 50)]
    pub ack_batch_size: usize,

    /// How often to flush pending acknowledgments
    #[arg(long, env = "OBSCURA_WS_ACK_FLUSH_INTERVAL_MS", default_value_t = 500)]
    pub ack_flush_interval_ms: u64,
}

#[derive(Clone, Debug, Args)]
pub struct S3Config {
    /// S3 bucket name
    #[arg(long, env = "OBSCURA_S3_BUCKET")]
    pub bucket: String,

    /// S3 region
    #[arg(long, env = "OBSCURA_S3_REGION", default_value = "us-east-1")]
    pub region: String,

    /// Custom S3 endpoint (useful for MinIO)
    #[arg(long, env = "OBSCURA_S3_ENDPOINT")]
    pub endpoint: Option<String>,

    /// S3 access key
    #[arg(long, env = "OBSCURA_S3_ACCESS_KEY")]
    pub access_key: Option<String>,

    /// S3 secret key
    #[arg(long, env = "OBSCURA_S3_SECRET_KEY")]
    pub secret_key: Option<String>,

    /// Force path style (required for many MinIO setups: http://host/bucket/key)
    #[arg(long, env = "OBSCURA_S3_FORCE_PATH_STYLE", default_value_t = false)]
    pub force_path_style: bool,

    /// Max attachment size in bytes (Default: 50MB)
    #[arg(long, env = "OBSCURA_S3_MAX_SIZE_BYTES", default_value_t = 52_428_800)]
    pub attachment_max_size_bytes: usize,
}

impl Config {
    pub fn load() -> Self {
        Self::parse()
    }
}
