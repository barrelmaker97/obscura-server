use crate::adapters::database::DbPool;
use crate::adapters::database::message_repo::MessageRepository;
use crate::config::MessagingConfig;
use crate::error::AppError;
use opentelemetry::{global, metrics::Counter};
use std::time::Duration;
use tracing::Instrument;

#[derive(Clone, Debug)]
struct Metrics {
    inbox_overflow: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            inbox_overflow: meter
                .u64_counter("messaging_inbox_overflow_total")
                .with_description("Total messages deleted due to inbox overflow")
                .build(),
        }
    }
}

#[derive(Debug)]
pub struct MessageCleanupWorker {
    pool: DbPool,
    repo: MessageRepository,
    config: MessagingConfig,
    metrics: Metrics,
}

impl MessageCleanupWorker {
    #[must_use]
    pub fn new(pool: DbPool, repo: MessageRepository, config: MessagingConfig) -> Self {
        Self { pool, repo, config, metrics: Metrics::new() }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.cleanup_interval_secs));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.perform_cleanup()
                        .instrument(tracing::info_span!("message_cleanup_iteration"))
                        .await
                    {
                        tracing::error!(error = ?e, "Message cleanup iteration failed");
                    }
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Message cleanup loop shutting down...");
    }

    /// Periodically cleans up expired messages and enforces inbox limits.
    ///
    /// # Errors
    /// Returns an error if the database connection or query fails.
    #[tracing::instrument(
        skip(self),
        err,
        fields(expired_deleted = tracing::field::Empty, overflow_deleted = tracing::field::Empty)
    )]
    pub async fn perform_cleanup(&self) -> Result<(), AppError> {
        tracing::debug!("Running message cleanup (expiry + limits)...");

        // Delete messages exceeding TTL
        let res_expiry = if let Ok(mut conn) = self.pool.acquire().await {
            self.repo.delete_expired(&mut conn).await
        } else {
            Err(AppError::Internal)
        };

        match res_expiry {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count = %count, "Deleted expired messages");
                    tracing::Span::current().record("expired_deleted", count);
                }
            }
            Err(e) => tracing::error!(error = ?e, "Cleanup error (expiry)"),
        }

        // Enforce global inbox size limits (prune oldest messages)
        let res_overflow = if let Ok(mut conn) = self.pool.acquire().await {
            self.repo.delete_global_overflow(&mut conn, self.config.max_inbox_size).await
        } else {
            Err(AppError::Internal)
        };

        match res_overflow {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count = %count, "Pruned overflow messages");
                    self.metrics.inbox_overflow.add(count, &[]);
                    tracing::Span::current().record("overflow_deleted", count);
                }
            }
            Err(e) => tracing::error!(error = ?e, "Cleanup error (overflow)"),
        }

        Ok(())
    }
}
