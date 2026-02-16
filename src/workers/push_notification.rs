use crate::adapters::database::DbPool;
use crate::adapters::database::push_token_repo::PushTokenRepository;
use crate::adapters::push::{PushError, PushProvider};
use crate::adapters::redis::NotificationRepository;
use crate::config::NotificationConfig;
use opentelemetry::{KeyValue, global, metrics::Counter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tracing::Instrument;

#[derive(Clone, Debug)]
struct Metrics {
    sent: Counter<u64>,
    errors: Counter<u64>,
    invalidated_tokens: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            sent: meter
                .u64_counter("push_sent_total")
                .with_description("Total number of push notifications successfully sent")
                .build(),
            errors: meter
                .u64_counter("push_errors_total")
                .with_description("Total number of push notification delivery errors")
                .build(),
            invalidated_tokens: meter
                .u64_counter("push_invalidated_tokens_total")
                .with_description("Total number of push tokens removed due to being unregistered")
                .build(),
        }
    }
}

#[derive(Debug)]
pub struct PushNotificationWorker {
    pool: DbPool,
    repo: Arc<NotificationRepository>,
    provider: Arc<dyn PushProvider>,
    token_repo: PushTokenRepository,
    interval_secs: u64,
    visibility_timeout_secs: u64,
    janitor_interval_secs: u64,
    janitor_batch_size: usize,
    janitor_channel_capacity: usize,
    semaphore: Arc<Semaphore>,
    metrics: Metrics,
}

impl PushNotificationWorker {
    pub fn new(
        pool: DbPool,
        repo: Arc<NotificationRepository>,
        provider: Arc<dyn PushProvider>,
        token_repo: PushTokenRepository,
        config: &NotificationConfig,
    ) -> Self {
        Self {
            pool,
            repo,
            provider,
            token_repo,
            interval_secs: config.worker_interval_secs,
            visibility_timeout_secs: config.visibility_timeout_secs,
            janitor_interval_secs: config.janitor_interval_secs,
            janitor_batch_size: config.janitor_batch_size,
            janitor_channel_capacity: config.janitor_channel_capacity,
            semaphore: Arc::new(Semaphore::new(config.worker_concurrency)),
            metrics: Metrics::new(),
        }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.interval_secs));
        let (invalid_token_tx, mut invalid_token_rx) = mpsc::channel::<String>(self.janitor_channel_capacity);

        // Spawn Janitor task to handle batch token deletion
        let janitor_pool = self.pool.clone();
        let janitor_repo = self.token_repo.clone();
        let janitor_interval = self.janitor_interval_secs;
        let janitor_batch_size = self.janitor_batch_size;
        let janitor_task = tokio::spawn(
            async move {
                let mut batch = Vec::new();
                let mut flush_interval = tokio::time::interval(Duration::from_secs(janitor_interval));

                loop {
                    tokio::select! {
                        res = invalid_token_rx.recv() => {
                            if let Some(token) = res {
                                batch.push(token);
                                if batch.len() >= janitor_batch_size {
                                    Self::flush_invalid_tokens(&janitor_pool, &janitor_repo, &mut batch).await;
                                }
                            } else {
                                // Channel closed, perform final flush and exit
                                Self::flush_invalid_tokens(&janitor_pool, &janitor_repo, &mut batch).await;
                                break;
                            }
                        }
                        _ = flush_interval.tick() => {
                            Self::flush_invalid_tokens(&janitor_pool, &janitor_repo, &mut batch).await;
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("push_token_janitor")),
        );

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.process_due_jobs(invalid_token_tx.clone())
                        .instrument(tracing::debug_span!("push_notification_iteration"))
                        .await
                    {
                        tracing::error!(error = %e, "Failed to process due notification jobs");
                    }
                }
                _ = shutdown.changed() => break,
            }
        }

        tracing::info!("Notification worker shutting down...");
        // Dropping the last tx will cause the janitor to finish its final flush and exit
        drop(invalid_token_tx);
        let _ = janitor_task.await;
    }

    #[tracing::instrument(level = "debug", skip(pool, repo, batch))]
    async fn flush_invalid_tokens(pool: &DbPool, repo: &PushTokenRepository, batch: &mut Vec<String>) {
        if batch.is_empty() {
            return;
        }

        let count = batch.len();
        match pool.acquire().await {
            Ok(mut conn) => {
                if let Err(e) = repo.delete_tokens_batch(&mut conn, batch).await {
                    tracing::error!(error = %e, "Failed to delete invalid token batch");
                } else {
                    tracing::info!(count, "Successfully deleted invalid tokens in batch");
                    batch.clear();
                }
            }
            Err(e) => tracing::error!(error = %e, "Failed to acquire connection for janitor"),
        }
    }

    /// Processes a batch of due push notification jobs.
    ///
    /// # Errors
    /// Returns an error if the scheduler or database operation fails.
    #[tracing::instrument(level = "debug", skip(self, invalid_token_tx), name = "process_due_jobs", err)]
    pub async fn process_due_jobs(&self, invalid_token_tx: mpsc::Sender<String>) -> anyhow::Result<()> {
        let available = self.semaphore.available_permits();
        if available == 0 {
            return Ok(());
        }

        // We poll up to 'available' jobs, which is naturally capped by worker_concurrency
        let limit = available.cast_signed();
        // If the worker crashes, the job will become visible again after this period.
        let user_ids = self.repo.lease_due_jobs(limit, self.visibility_timeout_secs).await?;

        if user_ids.is_empty() {
            tracing::debug!("No due push notification jobs found");
            return Ok(());
        }

        tracing::info!(count = user_ids.len(), "Processing leased push notifications");

        // 1. Batch lookup tokens for all users
        let user_token_pairs = {
            let mut conn = self.pool.acquire().await?;
            self.token_repo.find_tokens_for_users(&mut conn, &user_ids).await?
        };

        // 2. Dispatch concurrently, bounded by the semaphore
        for (user_id, token) in user_token_pairs {
            let provider = Arc::clone(&self.provider);
            let repo = Arc::clone(&self.repo);
            let metrics = self.metrics.clone();
            let tx = invalid_token_tx.clone();

            // Acquire a permit before spawning.
            let permit = Arc::clone(&self.semaphore)
                .acquire_owned()
                .await
                .map_err(|e| anyhow::anyhow!("Semaphore closed: {e}"))?;

            tokio::spawn(
                async move {
                    let _permit = permit;

                    match provider.send_push(&token).await {
                        Ok(()) => {
                            tracing::debug!(token = %token, "Push notification sent successfully");
                            metrics.sent.add(1, &[]);
                            // Success: Remove job from Redis
                            let _ = repo.delete_job(user_id).await;
                        }
                        Err(PushError::Unregistered) => {
                            tracing::info!(token = %token, "Token unregistered, reporting to janitor");
                            metrics.invalidated_tokens.add(1, &[]);

                            // Definitively failed: Remove job from Redis anyway
                            let _ = repo.delete_job(user_id).await;

                            // Send to janitor for batch DB deletion
                            let _ = tx.send(token).await;
                        }
                        Err(PushError::QuotaExceeded) => {
                            tracing::warn!("Push quota exceeded, allowing visibility timeout to trigger retry");
                            metrics.errors.add(1, &[KeyValue::new("reason", "quota_exceeded")]);
                            // We do NOT delete the job; it will be retried when the lease expires.
                        }
                        Err(PushError::Other(e)) => {
                            tracing::error!(error = %e, token = %token, "Failed to send push notification, will retry");
                            metrics.errors.add(1, &[KeyValue::new("reason", "other")]);
                            // We do NOT delete the job; it will be retried when the lease expires.
                        }
                    }
                }
                .instrument(tracing::debug_span!("dispatch_push", %user_id)),
            );
        }

        Ok(())
    }
}
