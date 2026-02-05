use crate::config::HealthConfig;
use crate::storage::DbPool;
use aws_sdk_s3::Client;
use opentelemetry::{KeyValue, global, metrics::Gauge};
use std::time::Duration;
use tokio::time::timeout;

#[derive(Clone)]
pub struct HealthMetrics {
    pub health_status: Gauge<i64>,
}

impl HealthMetrics {
    pub fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            health_status: meter
                .i64_gauge("health_status")
                .with_description("Status of health checks (1 for ok, 0 for error)")
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
        
        match timeout(db_timeout, sqlx::query("SELECT 1").execute(&self.pool)).await {
            Ok(Ok(_)) => {
                self.metrics.health_status.record(1, &[KeyValue::new("component", "database")]);
                Ok(())
            }
            Ok(Err(e)) => {
                self.metrics.health_status.record(0, &[KeyValue::new("component", "database")]);
                Err(format!("Database connection failed: {:?}", e))
            }
            Err(_) => {
                self.metrics.health_status.record(0, &[KeyValue::new("component", "database")]);
                Err("Database connection timed out".to_string())
            }
        }
    }

    pub async fn check_storage(&self) -> Result<(), String> {
        let storage_timeout = Duration::from_millis(self.config.storage_timeout_ms);

        match timeout(storage_timeout, self.s3_client.head_bucket().bucket(&self.storage_bucket).send()).await {
            Ok(Ok(_)) => {
                self.metrics.health_status.record(1, &[KeyValue::new("component", "storage")]);
                Ok(())
            }
            Ok(Err(e)) => {
                self.metrics.health_status.record(0, &[KeyValue::new("component", "storage")]);
                Err(format!("Storage connection failed for bucket {}: {:?}", self.storage_bucket, e))
            }
            Err(_) => {
                self.metrics.health_status.record(0, &[KeyValue::new("component", "storage")]);
                Err("Storage connection timed out".to_string())
            }
        }
    }
}