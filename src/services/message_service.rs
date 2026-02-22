use crate::adapters::database::DbPool;
use crate::adapters::database::message_repo::MessageRepository;
use crate::adapters::redis::IdempotencyRepository;
use crate::config::MessagingConfig;
use crate::domain::message::Message;
use crate::domain::notification::UserEvent;
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::{SendMessageResponse, OutgoingMessage, send_message_response};
use crate::services::notification_service::NotificationService;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
};
use prost::Message as _;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) struct Metrics {
    pub(crate) sent_total: Counter<u64>,
    pub(crate) fetch_batch_size: Histogram<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            sent_total: meter
                .u64_counter("obscura_messages_sent_total")
                .with_description("Total messages successfully sent")
                .build(),
            fetch_batch_size: meter
                .u64_histogram("obscura_message_fetch_batch_size")
                .with_description("Number of messages fetched in a single batch")
                .build(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MessageService {
    pool: DbPool,
    repo: MessageRepository,
    idempotency_repo: IdempotencyRepository,
    notifier: NotificationService,
    config: MessagingConfig,
    ttl_days: i64,
    metrics: Metrics,
}

impl MessageService {
    #[must_use]
    pub fn new(
        pool: DbPool,
        repo: MessageRepository,
        idempotency_repo: IdempotencyRepository,
        notifier: NotificationService,
        config: MessagingConfig,
        ttl_days: i64,
    ) -> Self {
        Self { pool, repo, idempotency_repo, notifier, config, ttl_days, metrics: Metrics::new() }
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
        client_message_id: Option<Uuid>,
        message_type: i32,
        content: Vec<u8>,
    ) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        match self
            .repo
            .create(
                &mut conn,
                sender_id,
                recipient_id,
                client_message_id,
                message_type,
                content,
                self.ttl_days,
            )
            .await
        {
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

    /// Sends a batch of messages with idempotency support.
    ///
    /// # Errors
    /// Returns `AppError::Internal` if idempotency caching fails.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self, messages),
        fields(sender_id = %sender_id, count = messages.len())
    )]
    pub async fn send_batch(
        &self,
        sender_id: Uuid,
        idempotency_key: Option<Uuid>,
        messages: Vec<OutgoingMessage>,
    ) -> Result<SendMessageResponse> {
        // 1. Check Idempotency
        if let Some(key) = idempotency_key {
            if let Ok(Some(cached)) = self.idempotency_repo.get_response(&key.to_string()).await {
                tracing::info!(%key, "Returning cached idempotency response");
                return SendMessageResponse::decode(cached.as_slice())
                    .map_err(|e| AppError::InternalMsg(format!("Failed to decode cached idempotency response: {e}")));
            }
        }

        let mut failed_messages = Vec::new();

        // 2. Process Batch
        for outgoing in messages {
            let recipient_id = match Uuid::parse_str(&outgoing.recipient_id) {
                Ok(id) => id,
                Err(_) => {
                    failed_messages.push(send_message_response::FailedMessage {
                        client_message_id: outgoing.client_message_id.clone(),
                        error_code: send_message_response::ErrorCode::UserNotFound as i32,
                        error_message: "Invalid recipient UUID".to_string(),
                    });
                    continue;
                }
            };

            let client_message_id = match Uuid::parse_str(&outgoing.client_message_id) {
                Ok(id) => Some(id),
                Err(_) => None, // Or could fail validation
            };

            let Some(msg) = outgoing.message else {
                failed_messages.push(send_message_response::FailedMessage {
                    client_message_id: outgoing.client_message_id.clone(),
                    error_code: send_message_response::ErrorCode::Unspecified as i32,
                    error_message: "Missing EncryptedMessage payload".to_string(),
                });
                continue;
            };

            match self
                .send_message(
                    sender_id,
                    recipient_id,
                    client_message_id,
                    msg.r#type,
                    msg.content,
                )
                .await
            {
                Ok(_) => {}
                Err(AppError::NotFound) => {
                    failed_messages.push(send_message_response::FailedMessage {
                        client_message_id: outgoing.client_message_id,
                        error_code: send_message_response::ErrorCode::UserNotFound as i32,
                        error_message: "Recipient not found".to_string(),
                    });
                }
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to send message in batch");
                    failed_messages.push(send_message_response::FailedMessage {
                        client_message_id: outgoing.client_message_id,
                        error_code: send_message_response::ErrorCode::Unspecified as i32,
                        error_message: format!("Internal error: {e}"),
                    });
                }
            }
        }

        let response = SendMessageResponse { failed_messages };

        // 3. Cache Result
        if let Some(key) = idempotency_key {
            let encoded = response.encode_to_vec();
            if let Err(e) = self.idempotency_repo.save_response(&key.to_string(), &encoded, 86400).await {
                tracing::error!(error = %e, "Failed to cache idempotency response");
            }
        }

        Ok(response)
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
    pub async fn delete_batch(&self, user_id: Uuid, message_ids: &[Uuid]) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        self.repo.delete_batch(&mut conn, user_id, message_ids).await
    }

    #[must_use]
    pub(crate) const fn batch_limit(&self) -> i64 {
        self.config.batch_limit
    }
}
