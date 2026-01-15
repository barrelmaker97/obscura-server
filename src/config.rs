use clap::Parser;

const DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 10;
const DEFAULT_RATE_LIMIT_BURST: u32 = 20;
const DEFAULT_SERVER_HOST: &str = "0.0.0.0";
const DEFAULT_SERVER_PORT: u16 = 3000;
const DEFAULT_MESSAGE_TTL_DAYS: i64 = 30;
const DEFAULT_MAX_INBOX_SIZE: i64 = 1000;
const DEFAULT_MESSAGE_CLEANUP_INTERVAL_SECS: u64 = 300;
const DEFAULT_NOTIFICATION_GC_INTERVAL_SECS: u64 = 60;
const DEFAULT_NOTIFICATION_CHANNEL_CAPACITY: usize = 16;
const DEFAULT_TRUSTED_PROXIES: &str = "10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,127.0.0.1/32";

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    #[arg(long, env)]
    pub database_url: String,

    #[arg(long, env)]
    pub jwt_secret: String,

    #[arg(long, env, default_value_t = DEFAULT_RATE_LIMIT_PER_SECOND)]
    pub rate_limit_per_second: u32,

    #[arg(long, env, default_value_t = DEFAULT_RATE_LIMIT_BURST)]
    pub rate_limit_burst: u32,

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

    #[arg(long, env, default_value = DEFAULT_TRUSTED_PROXIES)]
    pub trusted_proxies: String,
}

impl Config {
    pub fn load() -> Self {
        Self::parse()
    }
}
