use crate::config::MessagingConfig;
use crate::services::notification::{Notifier, UserEvent};
use crate::domain::message::Message;
use crate::error::Result;
use crate::storage::DbPool;
use crate::storage::message_repo::MessageRepository;
use opentelemetry::{KeyValue, global, metrics::{Counter, Histogram}};
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone)]
struct MessageMetrics {
    messages_sent_total: Counter<u64>,
    messaging_fetch_batch_size: Histogram<u64>,
    messaging_inbox_overflow_total: Counter<u64>,
}

impl MessageMetrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            messages_sent_total: meter
                .u64_counter("messages_sent_total")
                .with_description("Total messages successfully sent")
                .build(),
            messaging_fetch_batch_size: meter
                .u64_histogram("messaging_fetch_batch_size")
                .with_description("Number of messages fetched in a single batch")
                .build(),
            messaging_inbox_overflow_total: meter
                .u64_counter("messaging_inbox_overflow_total")
                .with_description("Total messages deleted due to inbox overflow")
                .build(),
        }
    }
}

#[derive(Clone)]
pub struct MessageService {
    pool: DbPool,
    repo: MessageRepository,
    notifier: Arc<dyn Notifier>,
    config: MessagingConfig,
    ttl_days: i64,
    metrics: MessageMetrics,
}

impl MessageService {
    pub fn new(
        pool: DbPool,
        repo: MessageRepository,
        notifier: Arc<dyn Notifier>,
        config: MessagingConfig,
        ttl_days: i64,
    ) -> Self {
        Self {
            pool,
            repo,
            notifier,
            config,
            ttl_days,
            metrics: MessageMetrics::new(),
        }
    }

    #[tracing::instrument(
        err(level = "warn"),
        skip(self, content, sender_id, recipient_id),
        fields(recipient_id = %recipient_id)
    )]
    pub async fn send_message(&self, sender_id: Uuid, recipient_id: Uuid, message_type: i32, content: Vec<u8>) -> Result<()> {
        // Limits are enforced asynchronously by the background cleanup loop to optimize the send path.
        match self.repo.create(&self.pool, sender_id, recipient_id, message_type, content, self.ttl_days).await {
            Ok(_) => {
                tracing::debug!("Message stored for delivery");
                self.metrics.messages_sent_total.add(1, &[KeyValue::new("status", "success")]);

                self.notifier.notify(recipient_id, UserEvent::MessageReceived);
                Ok(())
            }
            Err(e) => {
                self.metrics.messages_sent_total.add(1, &[KeyValue::new("status", "failure")]);
                Err(e)
            }
        }
    }

    #[tracing::instrument(
        err,
        skip(self, content),
        fields(recipient_id = %recipient_id, message_type = %message_type)
    )]
    pub async fn enqueue_message(
        &self,
        sender_id: Uuid,
        recipient_id: Uuid,
        message_type: i32,
        content: Vec<u8>,
    ) -> Result<()> {
        self.repo.create(&self.pool, sender_id, recipient_id, message_type, content, self.ttl_days).await?;
        Ok(())
    }

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
        let messages = self.repo.fetch_pending_batch(&self.pool, recipient_id, cursor, limit).await?;

        self.metrics.messaging_fetch_batch_size.record(messages.len() as u64, &[]);

        Ok(messages)
    }

    #[tracing::instrument(
        err,
        skip(self, executor),
        fields(user_id = %user_id)
    )]
    pub async fn delete_all_for_user<'e, E>(&self, executor: E, user_id: Uuid) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        self.repo.delete_all_for_user(executor, user_id).await?;
        Ok(())
    }

    #[tracing::instrument(
        err,
        skip(self),
        fields(batch_count = message_ids.len())
    )]
    pub async fn delete_batch(&self, message_ids: &[Uuid]) -> Result<()> {
        self.repo.delete_batch(&self.pool, message_ids).await
    }

    pub fn batch_limit(&self) -> i64 {
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
                        match self.repo.delete_expired(&self.pool).await {
                            Ok(count) => {
                                if count > 0 {
                                    tracing::info!(count = %count, "Deleted expired messages");
                                }
                            }
                            Err(e) => tracing::error!(error = %e, "Cleanup loop error (expiry)"),
                        }

                        // Enforce global inbox size limits (prune oldest messages)
                        match self.repo.delete_global_overflow(&self.pool, self.config.max_inbox_size).await {
                            Ok(count) => {
                                if count > 0 {
                                    tracing::info!(count = %count, "Pruned overflow messages");
                                    self.metrics.messaging_inbox_overflow_total.add(count, &[]);
                                }
                            }
                            Err(e) => tracing::error!(error = %e, "Cleanup loop error (overflow)"),
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