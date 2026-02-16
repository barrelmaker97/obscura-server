use crate::adapters::database::DbPool;
use crate::adapters::database::push_token_repo::PushTokenRepository;
use crate::adapters::push::{PushError, PushProvider};
use crate::adapters::redis::NotificationRepository;
use crate::config::NotificationConfig;
use opentelemetry::{KeyValue, global, metrics::Counter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
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
    poll_limit: isize,
    interval_secs: u64,
    visibility_timeout_secs: u64,
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
            poll_limit: config.worker_poll_limit,
            interval_secs: config.worker_interval_secs,
            visibility_timeout_secs: config.visibility_timeout_secs,
            semaphore: Arc::new(Semaphore::new(config.worker_concurrency)),
            metrics: Metrics::new(),
        }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.interval_secs));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.process_due_jobs()
                        .instrument(tracing::info_span!("push_notification_iteration"))
                        .await
                    {
                        tracing::error!(error = %e, "Failed to process due notification jobs");
                    }
                }
                _ = shutdown.changed() => break,
            }
        }
        tracing::info!("Notification worker shutting down...");
    }

    /// Processes a batch of due push notification jobs.
    ///
    /// # Errors
    /// Returns an error if the scheduler or database operation fails.
    #[tracing::instrument(skip(self), name = "process_due_jobs", err)]
    pub async fn process_due_jobs(&self) -> anyhow::Result<()> {
        let available = self.semaphore.available_permits();
        if available == 0 {
            return Ok(());
        }

        let limit = (available as isize).min(self.poll_limit);
        // If the worker crashes, the job will become visible again after this period.
        let user_ids = self.repo.lease_due_jobs(limit, self.visibility_timeout_secs).await?;

        if user_ids.is_empty() {
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
            let pool = self.pool.clone();
            let token_repo = self.token_repo.clone();
            let repo = Arc::clone(&self.repo);
            let metrics = self.metrics.clone();

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
                            tracing::info!(token = %token, "Token unregistered, deleting from database");
                            metrics.invalidated_tokens.add(1, &[]);

                            // Definitively failed: Remove job from Redis anyway
                            let _ = repo.delete_job(user_id).await;

                            let conn_res = pool.acquire().await;
                            if let Ok(mut conn) = conn_res
                                && let Err(e) = token_repo.delete_token(&mut conn, &token).await
                            {
                                tracing::error!(error = %e, token = %token, "Failed to delete unregistered token");
                            }
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
