use crate::adapters::database::DbPool;
use crate::adapters::database::backup_repo::BackupRepository;
use crate::adapters::storage::ObjectStorage;
use crate::config::StorageConfig;
use crate::error::{AppError, Result};
use opentelemetry::{global, metrics::Counter};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use time::{Duration, OffsetDateTime};
use tracing::Instrument;

#[derive(Clone, Debug)]
struct Metrics {
    cleanup_runs: Counter<u64>,
    cleaned_items: Counter<u64>,
    errors: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            cleanup_runs: meter.u64_counter("obscura_backup_janitor_runs").build(),
            cleaned_items: meter.u64_counter("obscura_backup_janitor_cleaned").build(),
            errors: meter.u64_counter("obscura_backup_janitor_errors").build(),
        }
    }
}

#[derive(Clone)]
pub struct BackupCleanupWorker {
    pool: DbPool,
    repo: BackupRepository,
    storage: Arc<dyn ObjectStorage>,
    config: StorageConfig,
    metrics: Metrics,
}

impl std::fmt::Debug for BackupCleanupWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupCleanupWorker")
            .field("config", &self.config)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl BackupCleanupWorker {
    #[must_use]
    pub fn new(pool: DbPool, repo: BackupRepository, storage: Arc<dyn ObjectStorage>, config: StorageConfig) -> Self {
        Self { pool, repo, storage, config, metrics: Metrics::new() }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(StdDuration::from_secs(self.config.backup_janitor_interval_secs));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    async {
                        tracing::debug!("Running backup janitor...");
                        match self.cleanup_stale().await {
                            Ok(count) => {
                                if count > 0 {
                                    self.metrics.cleaned_items.add(count, &[]);
                                }
                                self.metrics.cleanup_runs.add(1, &[]);
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Backup janitor failed");
                                self.metrics.errors.add(1, &[]);
                            }
                        }
                    }
                    .instrument(tracing::info_span!("run_backup_janitor"))
                    .await;
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Backup janitor shutting down...");
    }

    async fn cleanup_stale(&self) -> Result<u64> {
        let mut total_cleaned = 0;
        let threshold = OffsetDateTime::now_utc() - Duration::minutes(self.config.backup_stale_threshold_mins);

        loop {
            let mut conn = self.pool.acquire().await.map_err(AppError::Database)?;
            let stale_backups = self.repo.fetch_stale_uploads(&mut conn, threshold, 50).await?;

            if stale_backups.is_empty() {
                break;
            }

            for backup in stale_backups {
                let user_id = backup.user_id;
                let pending_version = backup.pending_version.unwrap_or(0);

                if pending_version > 0 {
                    let key = format!("{}{}/v{}", self.config.backup_prefix, user_id, pending_version);

                    // Delete from storage
                    if let Err(e) = self.storage.delete(&key).await {
                        tracing::warn!(error = ?e, key = %key, "Failed to delete stale backup from storage");
                    }
                }

                if let Err(e) = self.repo.reset_stale(&mut conn, user_id).await {
                    tracing::error!(error = ?e, user_id = %user_id, "Failed to reset stale backup in DB");
                } else {
                    total_cleaned += 1;
                }
            }
        }

        Ok(total_cleaned)
    }
}
