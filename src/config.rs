use clap::{Args, Parser};
use ipnetwork::IpNetwork;

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Database connection URL
    #[arg(long, env = "OBSCURA_DATABASE_URL", default_value_t = Config::default().database_url)]
    pub database_url: String,

    /// Global time-to-live for messages and attachments in days
    #[arg(long, env = "OBSCURA_TTL_DAYS", default_value_t = Config::default().ttl_days)]
    pub ttl_days: i64,

    #[command(flatten)]
    pub server: ServerConfig,

    #[command(flatten)]
    pub auth: AuthConfig,

    #[command(flatten)]
    pub rate_limit: RateLimitConfig,

    #[command(flatten)]
    pub health: HealthConfig,

    #[command(flatten)]
    pub messaging: MessagingConfig,

    #[command(flatten)]
    pub notifications: NotificationConfig,

    #[command(flatten)]
    pub cache: CacheConfig,

    #[command(flatten)]
    pub websocket: WsConfig,

    #[command(flatten)]
    pub storage: StorageConfig,

    #[command(flatten)]
    pub telemetry: TelemetryConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "postgres://user:password@localhost/signal_server".to_string(),
            ttl_days: 30,
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            rate_limit: RateLimitConfig::default(),
            health: HealthConfig::default(),
            messaging: MessagingConfig::default(),
            notifications: NotificationConfig::default(),
            cache: CacheConfig::default(),
            websocket: WsConfig::default(),
            storage: StorageConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

impl Config {
    #[must_use]
    pub fn load() -> Self {
        Self::parse()
    }
}

#[derive(Clone, Debug, Args)]
pub struct CacheConfig {
    /// Valkey connection URL (e.g. redis://localhost:6379)
    #[arg(long = "cache-url", env = "OBSCURA_CACHE_URL", default_value_t = CacheConfig::default().url)]
    pub url: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self { url: "redis://localhost:6379".to_string() }
    }
}

#[derive(Clone, Debug, Default, clap::ValueEnum)]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Debug, Args)]
pub struct TelemetryConfig {
    /// OTLP Endpoint for traces and metrics (e.g. <http://localhost:4318>)
    /// If not set, OTLP export is disabled (logs only).
    #[arg(long, env = "OBSCURA_OTLP_ENDPOINT")]
    pub otlp_endpoint: Option<String>,

    /// Log format (text or json)
    #[arg(long, env = "OBSCURA_LOG_FORMAT", default_value_t = TelemetryConfig::default().log_format)]
    pub log_format: LogFormat,

    /// Trace sampling ratio (0.0 to 1.0)
    #[arg(long, env = "OBSCURA_TRACE_SAMPLING_RATIO", default_value_t = TelemetryConfig::default().trace_sampling_ratio)]
    pub trace_sampling_ratio: f64,

    /// Metric export interval in seconds
    #[arg(long, env = "OBSCURA_TELEMETRY_METRICS_EXPORT_INTERVAL_SECS", default_value_t = TelemetryConfig::default().metrics_export_interval_secs)]
    pub metrics_export_interval_secs: u64,

    /// OTLP export timeout in seconds
    #[arg(long, env = "OBSCURA_TELEMETRY_EXPORT_TIMEOUT_SECS", default_value_t = TelemetryConfig::default().export_timeout_secs)]
    pub export_timeout_secs: u64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: None,
            log_format: LogFormat::Text,
            trace_sampling_ratio: 1.0,
            metrics_export_interval_secs: 60,
            export_timeout_secs: 10,
        }
    }
}

impl std::fmt::Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Json => write!(f, "json"),
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct ServerConfig {
    /// Host to listen on
    #[arg(long = "server-host", env = "OBSCURA_HOST", default_value_t = ServerConfig::default().host)]
    pub host: String,

    /// Port to listen on
    #[arg(long = "server-port", env = "OBSCURA_PORT", default_value_t = ServerConfig::default().port)]
    pub port: u16,

    /// Management port for health checks and metrics
    #[arg(long, env = "OBSCURA_MGMT_PORT", default_value_t = ServerConfig::default().mgmt_port)]
    pub mgmt_port: u16,

    /// How long to wait for background tasks to finish during shutdown in seconds
    #[arg(long, env = "OBSCURA_SHUTDOWN_TIMEOUT_SECS", default_value_t = ServerConfig::default().shutdown_timeout_secs)]
    pub shutdown_timeout_secs: u64,

    /// Comma-separated list of CIDRs to trust for X-Forwarded-For IP extraction
    #[arg(
        long,
        env = "OBSCURA_TRUSTED_PROXIES",
        default_value = "10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,127.0.0.1/32",
        value_delimiter = ','
    )]
    pub trusted_proxies: Vec<IpNetwork>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            mgmt_port: 9090,
            shutdown_timeout_secs: 5,
            trusted_proxies: vec![
                "10.0.0.0/8".parse().expect("Invalid default CIDR for private network"),
                "172.16.0.0/12".parse().expect("Invalid default CIDR for private network"),
                "192.168.0.0/16".parse().expect("Invalid default CIDR for private network"),
                "127.0.0.1/32".parse().expect("Invalid default CIDR for localhost"),
            ],
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct AuthConfig {
    /// Secret key for JWT signing
    #[arg(long, env = "OBSCURA_JWT_SECRET", default_value_t = AuthConfig::default().jwt_secret)]
    pub jwt_secret: String,

    /// Access token time-to-live in seconds
    #[arg(long, env = "OBSCURA_ACCESS_TOKEN_TTL_SECS", default_value_t = AuthConfig::default().access_token_ttl_secs)]
    pub access_token_ttl_secs: u64,

    /// Refresh token time-to-live in days
    #[arg(long, env = "OBSCURA_REFRESH_TOKEN_TTL_DAYS", default_value_t = AuthConfig::default().refresh_token_ttl_days)]
    pub refresh_token_ttl_days: i64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: "change_me_in_production".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_days: 30,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct RateLimitConfig {
    /// Requests per second allowed for standard endpoints
    #[arg(long = "rate-limit-per-second", env = "OBSCURA_RATE_LIMIT_PER_SECOND", default_value_t = RateLimitConfig::default().per_second)]
    pub per_second: u32,

    /// Burst allowance for standard endpoints
    #[arg(long = "rate-limit-burst", env = "OBSCURA_RATE_LIMIT_BURST", default_value_t = RateLimitConfig::default().burst)]
    pub burst: u32,

    /// Stricter rate limit for expensive auth-related endpoints (register/login)
    #[arg(long = "auth-rate-limit-per-second", env = "OBSCURA_AUTH_RATE_LIMIT_PER_SECOND", default_value_t = RateLimitConfig::default().auth_per_second)]
    pub auth_per_second: u32,

    /// Burst allowance for expensive auth-related endpoints
    #[arg(long = "auth-rate-limit-burst", env = "OBSCURA_AUTH_RATE_LIMIT_BURST", default_value_t = RateLimitConfig::default().auth_burst)]
    pub auth_burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self { per_second: 10, burst: 20, auth_per_second: 1, auth_burst: 3 }
    }
}

#[derive(Clone, Debug, Args)]
pub struct MessagingConfig {
    /// Maximum number of messages in a user's inbox
    #[arg(long, env = "OBSCURA_MAX_INBOX_SIZE", default_value_t = MessagingConfig::default().max_inbox_size)]
    pub max_inbox_size: i64,

    /// How often to run the message cleanup task
    #[arg(
        long = "messaging-cleanup-interval-secs",
        id = "messaging_cleanup_interval_secs",
        env = "OBSCURA_MESSAGING_CLEANUP_INTERVAL_SECS",
        default_value_t = MessagingConfig::default().cleanup_interval_secs
    )]
    pub cleanup_interval_secs: u64,

    /// Maximum number of messages to process in a single batch
    #[arg(long, env = "OBSCURA_BATCH_LIMIT", default_value_t = MessagingConfig::default().batch_limit)]
    pub batch_limit: i64,

    /// Threshold of one-time prekeys to trigger a refill notification
    #[arg(long, env = "OBSCURA_PRE_KEY_REFILL_THRESHOLD", default_value_t = MessagingConfig::default().pre_key_refill_threshold)]
    pub pre_key_refill_threshold: i32,

    /// Maximum number of one-time prekeys allowed per user
    #[arg(long, env = "OBSCURA_MAX_PRE_KEYS", default_value_t = MessagingConfig::default().max_pre_keys)]
    pub max_pre_keys: i64,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            max_inbox_size: 1000,
            cleanup_interval_secs: 300,
            batch_limit: 50,
            pre_key_refill_threshold: 20,
            max_pre_keys: 100,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct NotificationConfig {
    /// How often to run the notification garbage collection
    #[arg(long, env = "OBSCURA_GC_INTERVAL_SECS", default_value_t = NotificationConfig::default().gc_interval_secs)]
    pub gc_interval_secs: u64,

    /// Capacity of the notification channel
    #[arg(long, env = "OBSCURA_CHANNEL_CAPACITY", default_value_t = NotificationConfig::default().channel_capacity)]
    pub channel_capacity: usize,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            gc_interval_secs: 60,
            channel_capacity: 16,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct WsConfig {
    /// Size of the outbound message buffer
    #[arg(long = "ws-outbound-buffer-size", env = "OBSCURA_WS_OUTBOUND_BUFFER_SIZE", default_value_t = WsConfig::default().outbound_buffer_size)]
    pub outbound_buffer_size: usize,

    /// Size of the acknowledgment buffer
    #[arg(long = "ws-ack-buffer-size", env = "OBSCURA_WS_ACK_BUFFER_SIZE", default_value_t = WsConfig::default().ack_buffer_size)]
    pub ack_buffer_size: usize,

    /// Number of acknowledgments to batch before flushing
    #[arg(long = "ws-ack-batch-size", env = "OBSCURA_WS_ACK_BATCH_SIZE", default_value_t = WsConfig::default().ack_batch_size)]
    pub ack_batch_size: usize,

    /// How often to flush pending acknowledgments
    #[arg(long = "ws-ack-flush-interval-ms", env = "OBSCURA_WS_ACK_FLUSH_INTERVAL_MS", default_value_t = WsConfig::default().ack_flush_interval_ms)]
    pub ack_flush_interval_ms: u64,
}

impl Default for WsConfig {
    fn default() -> Self {
        Self { outbound_buffer_size: 32, ack_buffer_size: 100, ack_batch_size: 50, ack_flush_interval_ms: 500 }
    }
}

#[derive(Clone, Debug, Args)]
pub struct StorageConfig {
    /// Storage bucket name
    #[arg(long = "storage-bucket", env = "OBSCURA_STORAGE_BUCKET", default_value_t = StorageConfig::default().bucket)]
    pub bucket: String,

    /// Storage region
    #[arg(long = "storage-region", env = "OBSCURA_STORAGE_REGION", default_value_t = StorageConfig::default().region)]
    pub region: String,

    /// Custom storage endpoint (useful for `MinIO` or other S3-compatible services)
    #[arg(long = "storage-endpoint", env = "OBSCURA_STORAGE_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Storage access key
    #[arg(long = "storage-access-key", env = "OBSCURA_STORAGE_ACCESS_KEY")]
    pub access_key: Option<String>,

    /// Storage secret key
    #[arg(long = "storage-secret-key", env = "OBSCURA_STORAGE_SECRET_KEY")]
    pub secret_key: Option<String>,

    /// Force path style (required for many `MinIO` setups: <http://host/bucket/key>)
    #[arg(long = "storage-force-path-style", env = "OBSCURA_STORAGE_FORCE_PATH_STYLE", default_value_t = StorageConfig::default().force_path_style)]
    pub force_path_style: bool,

    /// Max attachment size in bytes (Default: 50MB)
    #[arg(long = "storage-max-size-bytes", env = "OBSCURA_STORAGE_MAX_SIZE_BYTES", default_value_t = StorageConfig::default().attachment_max_size_bytes)]
    pub attachment_max_size_bytes: usize,

    /// How often to run the attachment cleanup task in seconds
    #[arg(
        long = "storage-cleanup-interval-secs",
        id = "storage_cleanup_interval_secs",
        env = "OBSCURA_STORAGE_CLEANUP_INTERVAL_SECS",
        default_value_t = StorageConfig::default().cleanup_interval_secs
    )]
    pub cleanup_interval_secs: u64,

    /// Maximum number of attachments to delete in a single batch
    #[arg(long = "storage-cleanup-batch-size", env = "OBSCURA_STORAGE_CLEANUP_BATCH_SIZE", default_value_t = StorageConfig::default().cleanup_batch_size)]
    pub cleanup_batch_size: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            bucket: "obscura-attachments".to_string(),
            region: "us-east-1".to_string(),
            endpoint: None,
            access_key: None,
            secret_key: None,
            force_path_style: false,
            attachment_max_size_bytes: 52_428_800,
            cleanup_interval_secs: 3600,
            cleanup_batch_size: 100,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct HealthConfig {
    /// Timeout for database health check in milliseconds
    #[arg(long, env = "OBSCURA_HEALTH_DB_TIMEOUT_MS", default_value_t = HealthConfig::default().db_timeout_ms)]
    pub db_timeout_ms: u64,

    /// Timeout for storage health check in milliseconds
    #[arg(long = "storage-timeout-ms", env = "OBSCURA_STORAGE_HEALTH_TIMEOUT_MS", default_value_t = HealthConfig::default().storage_timeout_ms)]
    pub storage_timeout_ms: u64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self { db_timeout_ms: 2000, storage_timeout_ms: 2000 }
    }
}
