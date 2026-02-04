use crate::config::HealthConfig;
use crate::storage::DbPool;
use aws_sdk_s3::Client;
use opentelemetry::{KeyValue, global, metrics::Histogram};
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[derive(Clone)]
pub struct HealthMetrics {
    pub health_check_duration_seconds: Histogram<f64>,
}

impl HealthMetrics {
    pub fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            health_check_duration_seconds: meter
                .f64_histogram("health_check_duration_seconds")
                .with_description("Duration of health checks")
                .build(),
        }
    }
}

impl Default for HealthMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct HealthService {
    pool: DbPool,
    s3_client: Client,
    storage_bucket: String,
    config: HealthConfig,
    metrics: HealthMetrics,
}

impl HealthService {
    pub fn new(pool: DbPool, s3_client: Client, storage_bucket: String, config: HealthConfig) -> Self {
        Self {
            pool,
            s3_client,
            storage_bucket,
            config,
            metrics: HealthMetrics::new(),
        }
    }

    pub async fn check_db(&self) -> Result<(), String> {
        let db_timeout = Duration::from_millis(self.config.db_timeout_ms);
        let start = Instant::now();
        
        let res = match timeout(db_timeout, sqlx::query("SELECT 1").execute(&self.pool)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("Database connection failed: {:?}", e)),
            Err(_) => Err("Database connection timed out".to_string()),
        };

        self.metrics.health_check_duration_seconds.record(
            start.elapsed().as_secs_f64(),
            &[KeyValue::new("component", "database")],
        );
        res
    }

    pub async fn check_storage(&self) -> Result<(), String> {
        let storage_timeout = Duration::from_millis(self.config.storage_timeout_ms);
        let start = Instant::now();

        let res = match timeout(storage_timeout, self.s3_client.head_bucket().bucket(&self.storage_bucket).send()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("Storage connection failed for bucket {}: {:?}", self.storage_bucket, e)),
            Err(_) => Err("Storage connection timed out".to_string()),
        };

        self.metrics.health_check_duration_seconds.record(
            start.elapsed().as_secs_f64(),
            &[KeyValue::new("component", "storage")],
        );
        res
    }
}