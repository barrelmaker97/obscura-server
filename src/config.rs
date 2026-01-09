use std::env;
use dotenvy::dotenv;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub rate_limit_per_second: u32,
    pub rate_limit_burst: u32,
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
        })
    }
}
