use crate::config::MessagingConfig;
use crate::domain::message::Message;
use crate::error::{AppError, Result};
use crate::services::notification::{NotificationService, UserEvent};
use crate::storage::DbPool;
use crate::storage::message_repo::MessageRepository;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
};
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct Metrics {
    sent_total: Counter<u64>,
    fetch_batch_size: Histogram<u64>,
    inbox_overflow_total: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            sent_total: meter
                .u64_counter("messages_sent_total")
                .with_description("Total messages successfully sent")
                .build(),
            fetch_batch_size: meter
                .u64_histogram("messaging_fetch_batch_size")
                .with_description("Number of messages fetched in a single batch")
                .build(),
            inbox_overflow_total: meter
                .u64_counter("messaging_inbox_overflow_total")
                .with_description("Total messages deleted due to inbox overflow")
                .build(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MessageService {
    pool: DbPool,
    repo: MessageRepository,
    notifier: Arc<dyn NotificationService>,
    config: MessagingConfig,
    ttl_days: i64,
    metrics: Metrics,
}

impl MessageService {
    pub fn new(
        pool: DbPool,
        repo: MessageRepository,
        notifier: Arc<dyn NotificationService>,
        config: MessagingConfig,
        ttl_days: i64,
    ) -> Self {
        Self { pool, repo, notifier, config, ttl_days, metrics: Metrics::new() }
    }

    /// Sends a message to a recipient.
    ///
    /// # Errors
    /// Returns `AppError::NotFound` if the recipient does not exist.
    /// Returns `AppError::Database` if the message cannot be stored.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self, content, sender_id, recipient_id),
        fields(recipient_id = %recipient_id)
    )]
    pub async fn send_message(
        &self,
        sender_id: Uuid,
        recipient_id: Uuid,
        message_type: i32,
        content: Vec<u8>,
    ) -> Result<()> {
        // Limits are enforced asynchronously by the background cleanup loop to optimize the send path.
        let mut conn = self.pool.acquire().await?;
        match self.repo.create(&mut conn, sender_id, recipient_id, message_type, content, self.ttl_days).await {
            Ok(_) => {
                tracing::debug!("Message stored for delivery");
                self.metrics.sent_total.add(1, &[KeyValue::new("status", "success")]);

                self.notifier.notify(recipient_id, UserEvent::MessageReceived).await;
                Ok(())
            }
            Err(e) => {
                self.metrics.sent_total.add(1, &[KeyValue::new("status", "failure")]);
                Err(e)
            }
        }
    }

    /// Fetches a batch of pending messages for a recipient.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the query fails.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self),
        fields(recipient_id = %recipient_id, batch_limit = %limit)
    )]
    pub async fn fetch_pending_batch(
        &self,
        recipient_id: Uuid,
        cursor: Option<(time::OffsetDateTime, Uuid)>,
        limit: i64,
    ) -> Result<Vec<Message>> {
        let mut conn = self.pool.acquire().await?;
        let messages = self.repo.fetch_pending_batch(&mut conn, recipient_id, cursor, limit).await?;

        self.metrics.fetch_batch_size.record(messages.len() as u64, &[]);

        Ok(messages)
    }

    /// Deletes a batch of messages.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the deletion fails.
    #[tracing::instrument(
        err,
        skip(self),
        fields(batch_count = message_ids.len())
    )]
    pub async fn delete_batch(&self, message_ids: &[Uuid]) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        self.repo.delete_batch(&mut conn, message_ids).await
    }

    #[must_use]
    pub(crate) const fn batch_limit(&self) -> i64 {
        self.config.batch_limit
    }

    /// Periodically cleans up expired messages and enforces inbox limits.
    pub async fn run_cleanup_loop(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.cleanup_interval_secs));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    async {
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
                                }
                            }
                            Err(e) => tracing::error!(error = ?e, "Cleanup loop error (expiry)"),
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
                                    self.metrics.inbox_overflow_total.add(count, &[]);
                                }
                            }
                            Err(e) => tracing::error!(error = ?e, "Cleanup loop error (overflow)"),
                        }
                    }
                    .instrument(tracing::info_span!("message_cleanup_iteration"))
                    .await;
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Message cleanup loop shutting down...");
    }
}
