use clap::{Args, Parser};
use ipnetwork::IpNetwork;

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Database connection URL
    #[arg(long, env)]
    pub database_url: String,

    /// Global time-to-live for messages and attachments in days
    #[arg(long, env, default_value_t = 30)]
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
    #[arg(long, env, default_value = "0.0.0.0")]
    pub host: String,

    /// Port to listen on
    #[arg(long, env = "PORT", default_value_t = 3000)]
    pub port: u16,

    /// Comma-separated list of CIDRs to trust for X-Forwarded-For IP extraction
    #[arg(
        long,
        env,
        default_value = "10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,127.0.0.1/32",
        value_delimiter = ','
    )]
    pub trusted_proxies: Vec<IpNetwork>,
}

#[derive(Clone, Debug, Args)]
pub struct AuthConfig {
    /// Secret key for JWT signing
    #[arg(long, env)]
    pub jwt_secret: String,

    /// Access token time-to-live in seconds
    #[arg(long, env, default_value_t = 900)]
    pub access_token_ttl_secs: u64,

    /// Refresh token time-to-live in days
    #[arg(long, env, default_value_t = 30)]
    pub refresh_token_ttl_days: i64,
}

#[derive(Clone, Debug, Args)]
pub struct RateLimitConfig {
    /// Requests per second allowed for standard endpoints
    #[arg(long, env, default_value_t = 10)]
    pub per_second: u32,

    /// Burst allowance for standard endpoints
    #[arg(long, env, default_value_t = 20)]
    pub burst: u32,

    /// Stricter rate limit for expensive auth-related endpoints (register/login)
    #[arg(long, env, default_value_t = 1)]
    pub auth_per_second: u32,

    /// Burst allowance for expensive auth-related endpoints
    #[arg(long, env, default_value_t = 3)]
    pub auth_burst: u32,
}

#[derive(Clone, Debug, Args)]
pub struct MessagingConfig {
    /// Maximum number of messages in a user's inbox
    #[arg(long, env, default_value_t = 1000)]
    pub max_inbox_size: i64,

    /// How often to run the message cleanup task
    #[arg(long, env, default_value_t = 300)]
    pub cleanup_interval_secs: u64,

    /// Maximum number of messages to process in a single batch
    #[arg(long, env, default_value_t = 50)]
    pub batch_limit: i64,
}

#[derive(Clone, Debug, Args)]
pub struct NotificationConfig {
    /// How often to run the notification garbage collection
    #[arg(long, env, default_value_t = 60)]
    pub gc_interval_secs: u64,

    /// Capacity of the notification channel
    #[arg(long, env, default_value_t = 16)]
    pub channel_capacity: usize,
}

#[derive(Clone, Debug, Args)]
pub struct WsConfig {
    /// Size of the outbound message buffer
    #[arg(long, env, default_value_t = 32)]
    pub outbound_buffer_size: usize,

    /// Size of the acknowledgment buffer
    #[arg(long, env, default_value_t = 100)]
    pub ack_buffer_size: usize,

    /// Number of acknowledgments to batch before flushing
    #[arg(long, env, default_value_t = 50)]
    pub ack_batch_size: usize,

    /// How often to flush pending acknowledgments
    #[arg(long, env, default_value_t = 500)]
    pub ack_flush_interval_ms: u64,
}

#[derive(Clone, Debug, Args)]
pub struct S3Config {
    /// S3 bucket name
    #[arg(long, env)]
    pub bucket: String,

    /// S3 region
    #[arg(long, env, default_value = "us-east-1")]
    pub region: String,

    /// Custom S3 endpoint (useful for MinIO)
    #[arg(long, env)]
    pub endpoint: Option<String>,

    /// S3 access key
    #[arg(long, env)]
    pub access_key: Option<String>,

    /// S3 secret_key
    #[arg(long, env)]
    pub secret_key: Option<String>,

    /// Force path style (required for many MinIO setups: http://host/bucket/key)
    #[arg(long, env, default_value_t = false)]
    pub force_path_style: bool,

    /// Max attachment size in bytes (Default: 50MB)
    #[arg(long, env, default_value_t = 52_428_800)]
    pub attachment_max_size_bytes: usize,
}

impl Config {
    pub fn load() -> Self {
        Self::parse()
    }
}
