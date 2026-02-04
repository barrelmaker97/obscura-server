use crate::config::MessagingConfig;
use crate::core::notification::{Notifier, UserEvent};
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::EncryptedMessage;
use crate::storage::DbPool;
use crate::storage::message_repo::MessageRepository;
use axum::body::Bytes;
use opentelemetry::{KeyValue, global, metrics::{Counter, Histogram}};
use prost::Message;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
struct MessageMetrics {
    messages_sent_total: Counter<u64>,
    messaging_fetch_batch_size: Histogram<u64>,
    messaging_inbox_full_events_total: Counter<u64>,
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
            messaging_inbox_full_events_total: meter
                .u64_counter("messaging_inbox_full_events_total")
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
        err,
        skip(self, body, sender_id, recipient_id),
        fields(sender.id = %sender_id, recipient.id = %recipient_id)
    )]
    pub async fn send_message(&self, sender_id: Uuid, recipient_id: Uuid, body: Bytes) -> Result<()> {
        // 1. Decode the EncryptedMessage protobuf to get type and content
        let msg = EncryptedMessage::decode(body)
            .map_err(|e| AppError::BadRequest(format!("Invalid EncryptedMessage protobuf: {}", e)))?;

        // 2. Store raw body directly (blind relay)
        // Optimization: We no longer check limits synchronously.
        // The background cleanup loop handles overflow.
        match self.repo.create(&self.pool, sender_id, recipient_id, msg.r#type, msg.content, self.ttl_days).await {
            Ok(_) => {
                tracing::debug!("Message stored for delivery");
                self.metrics.messages_sent_total.add(1, &[KeyValue::new("status", "success")]);

                // 3. Notify the user if they are connected
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
        fields(sender.id = %sender_id, recipient.id = %recipient_id, message.type = %message_type)
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
        err,
        skip(self),
        fields(recipient.id = %recipient_id, batch.limit = %limit)
    )]
    pub async fn fetch_pending_batch(
        &self,
        recipient_id: Uuid,
        cursor: Option<(time::OffsetDateTime, Uuid)>,
        limit: i64,
    ) -> Result<Vec<crate::core::message::Message>> {
        let messages = self.repo.fetch_pending_batch(&self.pool, recipient_id, cursor, limit).await?;

        self.metrics.messaging_fetch_batch_size.record(messages.len() as u64, &[]);

        Ok(messages)
    }

    #[tracing::instrument(
        err,
        skip(self),
        fields(batch.count = message_ids.len())
    )]
    pub async fn delete_batch(&self, message_ids: &[Uuid]) -> Result<()> {
        self.repo.delete_batch(&self.pool, message_ids).await
    }

    pub fn batch_limit(&self) -> i64 {
        self.config.batch_limit
    }

    /// Periodically cleans up expired messages and enforces inbox limits.
    #[tracing::instrument(skip(self, shutdown), name = "message_cleanup_task")]
    pub async fn run_cleanup_loop(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.cleanup_interval_secs));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    tracing::debug!("Running message cleanup (expiry + limits)...");

                    // 1. Delete Expired (TTL)
                    match self.repo.delete_expired(&self.pool).await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(count = %count, "Deleted expired messages");
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "Cleanup loop error (expiry)"),
                    }

                    // 2. Enforce Inbox Limits (Global Overflow)
                    // Limit to max_inbox_size messages per user
                    match self.repo.delete_global_overflow(&self.pool, self.config.max_inbox_size).await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(count = %count, "Pruned overflow messages");
                                self.metrics.messaging_inbox_full_events_total.add(count, &[]);
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "Cleanup loop error (overflow)"),
                    }
                }
                _ = shutdown.changed() => {}
            }
        }
        tracing::info!("Message cleanup loop shutting down...");
    }
}
