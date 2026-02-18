use clap::{Args, Parser};
use ipnetwork::IpNetwork;

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Global time-to-live for messages and attachments in days
    #[arg(long, env = "OBSCURA_TTL_DAYS", default_value_t = Config::default().ttl_days)]
    pub ttl_days: i64,

    #[command(flatten)]
    pub database: DatabaseConfig,

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
    pub pubsub: PubSubConfig,

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
            ttl_days: 30,
            database: DatabaseConfig::default(),
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            rate_limit: RateLimitConfig::default(),
            health: HealthConfig::default(),
            messaging: MessagingConfig::default(),
            notifications: NotificationConfig::default(),
            pubsub: PubSubConfig::default(),
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
pub struct DatabaseConfig {
    /// Database connection URL
    #[arg(
        long = "db-url",
        id = "database_url",
        env = "OBSCURA_DATABASE_URL",
        default_value_t = DatabaseConfig::default().url
    )]
    pub url: String,

    /// Maximum number of connections in the pool
    #[arg(long = "db-max-connections", env = "OBSCURA_DATABASE_MAX_CONNECTIONS", default_value_t = DatabaseConfig::default().max_connections)]
    pub max_connections: u32,

    /// Minimum number of connections to keep idle in the pool
    #[arg(long = "db-min-connections", env = "OBSCURA_DATABASE_MIN_CONNECTIONS", default_value_t = DatabaseConfig::default().min_connections)]
    pub min_connections: u32,

    /// Seconds to wait before timing out on acquiring a connection
    #[arg(long = "db-acquire-timeout-secs", env = "OBSCURA_DATABASE_ACQUIRE_TIMEOUT_SECS", default_value_t = DatabaseConfig::default().acquire_timeout_secs)]
    pub acquire_timeout_secs: u64,

    /// Seconds before an idle connection is closed
    #[arg(long = "db-idle-timeout-secs", env = "OBSCURA_DATABASE_IDLE_TIMEOUT_SECS", default_value_t = DatabaseConfig::default().idle_timeout_secs)]
    pub idle_timeout_secs: u64,

    /// Seconds before a connection is retired and replaced
    #[arg(long = "db-max-lifetime-secs", env = "OBSCURA_DATABASE_MAX_LIFETIME_SECS", default_value_t = DatabaseConfig::default().max_lifetime_secs)]
    pub max_lifetime_secs: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgres://user:password@localhost/signal_server".to_string(),
            max_connections: 20,
            min_connections: 5,
            acquire_timeout_secs: 3,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct PubSubConfig {
    /// `PubSub` connection URL (e.g. <redis://localhost:6379>)
    #[arg(
        long = "pubsub-url",
        id = "pubsub_url",
        env = "OBSCURA_PUBSUB_URL",
        default_value_t = PubSubConfig::default().url
    )]
    pub url: String,

    /// Minimum backoff time for `PubSub` reconnection in seconds
    #[arg(long = "pubsub-min-backoff-secs", env = "OBSCURA_PUBSUB_MIN_BACKOFF_SECS", default_value_t = PubSubConfig::default().min_backoff_secs)]
    pub min_backoff_secs: u64,

    /// Maximum backoff time for `PubSub` reconnection in seconds
    #[arg(long = "pubsub-max-backoff-secs", env = "OBSCURA_PUBSUB_MAX_BACKOFF_SECS", default_value_t = PubSubConfig::default().max_backoff_secs)]
    pub max_backoff_secs: u64,
}

impl Default for PubSubConfig {
    fn default() -> Self {
        Self { url: "redis://localhost:6379".to_string(), min_backoff_secs: 1, max_backoff_secs: 30 }
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
    #[arg(long, env = "OBSCURA_TELEMETRY_OTLP_ENDPOINT")]
    pub otlp_endpoint: Option<String>,

    /// Log format (text or json)
    #[arg(long, env = "OBSCURA_TELEMETRY_LOG_FORMAT", default_value_t = TelemetryConfig::default().log_format)]
    pub log_format: LogFormat,

    /// Trace sampling ratio (0.0 to 1.0)
    #[arg(long, env = "OBSCURA_TELEMETRY_TRACE_SAMPLING_RATIO", default_value_t = TelemetryConfig::default().trace_sampling_ratio)]
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
    #[arg(long = "server-host", env = "OBSCURA_SERVER_HOST", default_value_t = ServerConfig::default().host)]
    pub host: String,

    /// Port to listen on
    #[arg(long = "server-port", env = "OBSCURA_SERVER_PORT", default_value_t = ServerConfig::default().port)]
    pub port: u16,

    /// Management port for health checks and metrics
    #[arg(long, env = "OBSCURA_SERVER_MGMT_PORT", default_value_t = ServerConfig::default().mgmt_port)]
    pub mgmt_port: u16,

    /// How long to wait for background tasks to finish during shutdown in seconds
    #[arg(long, env = "OBSCURA_SERVER_SHUTDOWN_TIMEOUT_SECS", default_value_t = ServerConfig::default().shutdown_timeout_secs)]
    pub shutdown_timeout_secs: u64,

    /// Comma-separated list of CIDRs to trust for X-Forwarded-For IP extraction
    #[arg(
        long,
        env = "OBSCURA_SERVER_TRUSTED_PROXIES",
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
    #[arg(long, env = "OBSCURA_AUTH_JWT_SECRET", default_value_t = AuthConfig::default().jwt_secret)]
    pub jwt_secret: String,

    /// Access token time-to-live in seconds
    #[arg(long, env = "OBSCURA_AUTH_TOKEN_TTL_SECS", default_value_t = AuthConfig::default().access_token_ttl_secs)]
    pub access_token_ttl_secs: u64,

    /// Refresh token time-to-live in days
    #[arg(long, env = "OBSCURA_AUTH_REFRESH_TOKEN_TTL_DAYS", default_value_t = AuthConfig::default().refresh_token_ttl_days)]
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
    #[arg(long = "auth-rate-limit-per-second", env = "OBSCURA_RATE_LIMIT_AUTH_PER_SECOND", default_value_t = RateLimitConfig::default().auth_per_second)]
    pub auth_per_second: u32,

    /// Burst allowance for expensive auth-related endpoints
    #[arg(long = "auth-rate-limit-burst", env = "OBSCURA_RATE_LIMIT_AUTH_BURST", default_value_t = RateLimitConfig::default().auth_burst)]
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
    #[arg(long, env = "OBSCURA_MESSAGING_INBOX_MAX_SIZE", default_value_t = MessagingConfig::default().max_inbox_size)]
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
    #[arg(long, env = "OBSCURA_MESSAGING_BATCH_LIMIT", default_value_t = MessagingConfig::default().batch_limit)]
    pub batch_limit: i64,

    /// Threshold of one-time prekeys to trigger a refill notification
    #[arg(long, env = "OBSCURA_PRE_KEY_REFILL_THRESHOLD", default_value_t = MessagingConfig::default().pre_key_refill_threshold)]
    pub pre_key_refill_threshold: i32,

    /// Maximum number of one-time prekeys allowed per user
    #[arg(long, env = "OBSCURA_PRE_KEYS_MAX", default_value_t = MessagingConfig::default().max_pre_keys)]
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
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_GC_INTERVAL_SECS", default_value_t = NotificationConfig::default().gc_interval_secs)]
    pub gc_interval_secs: u64,

    /// Capacity of the global notification dispatcher channel
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_GLOBAL_CHANNEL_CAPACITY", default_value_t = NotificationConfig::default().global_channel_capacity)]
    pub global_channel_capacity: usize,

    /// Capacity of the per-user notification channel
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_USER_CHANNEL_CAPACITY", default_value_t = NotificationConfig::default().user_channel_capacity)]
    pub user_channel_capacity: usize,

    /// Delay in seconds before a push notification is sent as a fallback
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_PUSH_DELAY_SECS", default_value_t = NotificationConfig::default().push_delay_secs)]
    pub push_delay_secs: u64,

    /// Interval in seconds for the notification worker to poll for due jobs
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_WORKER_INTERVAL_SECS", default_value_t = NotificationConfig::default().worker_interval_secs)]
    pub worker_interval_secs: u64,

    /// Maximum number of concurrent push notification requests (also used as Redis poll limit)
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_WORKER_CONCURRENCY", default_value_t = NotificationConfig::default().worker_concurrency)]
    pub worker_concurrency: usize,

    /// Redis key for the push notification job queue
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_PUSH_QUEUE_KEY", default_value_t = NotificationConfig::default().push_queue_key)]
    pub push_queue_key: String,

    /// Redis `PubSub` channel prefix for user notifications
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_CHANNEL_PREFIX", default_value_t = NotificationConfig::default().channel_prefix)]
    pub channel_prefix: String,

    /// How long a push job is leased by a worker in seconds
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_VISIBILITY_TIMEOUT_SECS", default_value_t = NotificationConfig::default().visibility_timeout_secs)]
    pub visibility_timeout_secs: u64,

    /// How often the invalid token janitor flushes to the database in seconds
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_JANITOR_INTERVAL_SECS", default_value_t = NotificationConfig::default().janitor_interval_secs)]
    pub janitor_interval_secs: u64,

    /// Maximum number of invalid tokens to delete in a single batch
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_JANITOR_BATCH_SIZE", default_value_t = NotificationConfig::default().janitor_batch_size)]
    pub janitor_batch_size: usize,

    /// Capacity of the invalid token janitor channel
    #[arg(long, env = "OBSCURA_NOTIFICATIONS_JANITOR_CHANNEL_CAPACITY", default_value_t = NotificationConfig::default().janitor_channel_capacity)]
    pub janitor_channel_capacity: usize,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            gc_interval_secs: 60,
            global_channel_capacity: 1024,
            user_channel_capacity: 64,
            push_delay_secs: 10,
            worker_interval_secs: 1,
            worker_concurrency: 100,
            push_queue_key: "jobs:push_notifications".to_string(),
            channel_prefix: "user:".to_string(),
            visibility_timeout_secs: 30,
            janitor_interval_secs: 5,
            janitor_batch_size: 50,
            janitor_channel_capacity: 256,
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
    #[arg(long = "storage-timeout-ms", env = "OBSCURA_HEALTH_STORAGE_TIMEOUT_MS", default_value_t = HealthConfig::default().storage_timeout_ms)]
    pub storage_timeout_ms: u64,

    /// Timeout for pubsub health check in milliseconds
    #[arg(long = "pubsub-timeout-ms", env = "OBSCURA_HEALTH_PUBSUB_TIMEOUT_MS", default_value_t = HealthConfig::default().pubsub_timeout_ms)]
    pub pubsub_timeout_ms: u64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self { db_timeout_ms: 2000, storage_timeout_ms: 2000, pubsub_timeout_ms: 2000 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Config::command().debug_assert();
    }
}
