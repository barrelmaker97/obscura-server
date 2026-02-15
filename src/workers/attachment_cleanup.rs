use crate::adapters::database::DbPool;
use crate::adapters::database::attachment_repo::AttachmentRepository;
use crate::config::StorageConfig;
use crate::error::Result;
use aws_sdk_s3::Client;
use opentelemetry::{global, metrics::Counter};
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
                .u64_counter("attachments_deleted_total")
                .with_description("Total number of expired attachments deleted from storage")
                .build(),
            errors: meter
                .u64_counter("attachments_cleanup_errors_total")
                .with_description("Total number of errors encountered during attachment cleanup")
                .build(),
        }
    }
}

#[derive(Debug)]
pub struct AttachmentCleanupWorker {
    pool: DbPool,
    repo: AttachmentRepository,
    s3_client: Client,
    config: StorageConfig,
    metrics: Metrics,
}

impl AttachmentCleanupWorker {
    #[must_use]
    pub fn new(pool: DbPool, repo: AttachmentRepository, s3_client: Client, config: StorageConfig) -> Self {
        Self { pool, repo, s3_client, config, metrics: Metrics::new() }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let interval = StdDuration::from_secs(self.config.cleanup_interval_secs);
        let mut next_tick = tokio::time::Instant::now() + interval;

        while !*shutdown.borrow() {
            tokio::select! {
                () = tokio::time::sleep_until(next_tick) => {
                    async {
                        tracing::debug!("Running attachment cleanup...");

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
                    .instrument(tracing::info_span!("attachment_cleanup_iteration"))
                    .await;
                    next_tick = tokio::time::Instant::now() + interval;
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Attachment cleanup loop shutting down...");
    }

    #[tracing::instrument(
        err,
        skip(self),
        fields(batch_count = tracing::field::Empty)
    )]
    async fn cleanup_batch(&self) -> Result<u64> {
        let mut total_deleted = 0;
        loop {
            // Fetch expired attachments
            let mut conn = self.pool.acquire().await?;
            let ids = self
                .repo
                .fetch_expired(&mut conn, i64::try_from(self.config.cleanup_batch_size).unwrap_or(i64::MAX))
                .await?;

            if ids.is_empty() {
                break;
            }

            tracing::Span::current().record("batch.count", ids.len());
            tracing::info!(count = %ids.len(), "Found expired attachments to delete");

            let count = ids.len();
            for id in ids {
                let key = id.to_string();
                let res: Result<bool> = async {
                    // Delete object from S3 first to avoid orphaned files
                    let res = self.s3_client.delete_object().bucket(&self.config.bucket).key(&key).send().await;

                    match res {
                        Ok(_) => {}
                        Err(aws_sdk_s3::error::SdkError::ServiceError(e)) => {
                            tracing::warn!(error = ?e, key = %key, "S3 delete error");
                            return Ok(false); // Skip DB delete if S3 failed
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, key = %key, "S3 network/transport error");
                            return Ok(false);
                        }
                    }

                    // Only delete from DB if S3 deletion was successful
                    let mut conn = self.pool.acquire().await?;
                    self.repo.delete(&mut conn, id).await?;
                    Ok(true)
                }
                .instrument(tracing::info_span!("delete_attachment", "attachment.id" = %id))
                .await;

                if matches!(res, Ok(true)) {
                    total_deleted += 1;
                }
            }
            tracing::info!(deleted_count = %count, "Attachment cleanup batch completed successfully");
        }
        Ok(total_deleted)
    }
}
