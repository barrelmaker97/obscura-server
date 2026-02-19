use crate::adapters::database::DbPool;
use crate::adapters::database::attachment_repo::AttachmentRepository;
use crate::adapters::storage::ObjectStorage;
use crate::config::AttachmentConfig;
use crate::error::Result;
use opentelemetry::{global, metrics::Counter};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tracing::Instrument;

#[derive(Clone, Debug)]
struct Metrics {
    deleted: Counter<u64>,
    errors: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            deleted: meter
                .u64_counter("obscura_attachments_deleted_total")
                .with_description("Total number of expired attachments deleted from storage")
                .build(),
            errors: meter
                .u64_counter("obscura_attachment_cleanup_errors_total")
                .with_description("Total number of errors encountered during attachment cleanup")
                .build(),
        }
    }
}

#[derive(Clone)]
pub struct AttachmentCleanupWorker {
    pool: DbPool,
    repo: AttachmentRepository,
    storage: Arc<dyn ObjectStorage>,
    attachment_config: AttachmentConfig,
    metrics: Metrics,
}

impl std::fmt::Debug for AttachmentCleanupWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttachmentCleanupWorker")
            .field("attachment_config", &self.attachment_config)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl AttachmentCleanupWorker {
    #[must_use]
    pub fn new(
        pool: DbPool,
        repo: AttachmentRepository,
        storage: Arc<dyn ObjectStorage>,
        attachment_config: AttachmentConfig,
    ) -> Self {
        Self { pool, repo, storage, attachment_config, metrics: Metrics::new() }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(StdDuration::from_secs(self.attachment_config.cleanup_interval_secs));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    async {
                        tracing::debug!("Running attachment cleanup cycle...");

                        match self.cleanup_batch().await {
                            Ok(count) => {
                                if count > 0 {
                                    self.metrics.deleted.add(count, &[]);
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Attachment cleanup cycle failed");
                                self.metrics.errors.add(1, &[]);
                            }
                        }
                    }
                    .instrument(tracing::info_span!("run_attachment_cleanup"))
                    .await;
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Attachment cleanup loop shutting down...");
    }

    #[tracing::instrument(
        err,
        skip(self),
        fields(total_deleted = tracing::field::Empty)
    )]
    async fn cleanup_batch(&self) -> Result<u64> {
        let mut total_deleted = 0;
        loop {
            // Fetch expired attachments
            let mut conn = self.pool.acquire().await?;
            let ids = self
                .repo
                .fetch_expired(&mut conn, i64::try_from(self.attachment_config.cleanup_batch_size).unwrap_or(i64::MAX))
                .await?;

            if ids.is_empty() {
                break;
            }

            tracing::info!(count = %ids.len(), "Found expired attachments to delete");

            let count = ids.len();
            for id in ids {
                let key = format!("{}{}", self.attachment_config.prefix, id);
                let res: Result<bool> = async {
                    // Delete object from Storage first to avoid orphaned files
                    let res = self.storage.delete(&key).await;

                    if let Err(e) = res {
                        tracing::warn!(error = ?e, key = %key, "Storage delete error");
                        return Ok(false); // Skip DB delete if storage failed
                    }

                    // Only delete from DB if storage deletion was successful
                    let mut conn = self.pool.acquire().await?;
                    self.repo.delete(&mut conn, id).await?;
                    Ok(true)
                }
                .instrument(tracing::debug_span!("delete_attachment", attachment_id = %id))
                .await;

                if matches!(res, Ok(true)) {
                    total_deleted += 1;
                }
            }
            tracing::info!(deleted_count = %count, "Attachment cleanup batch completed successfully");
        }

        if total_deleted > 0 {
            tracing::Span::current().record("total_deleted", total_deleted);
        }

        Ok(total_deleted)
    }
}
