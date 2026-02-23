use crate::adapters::database::DbPool;
use crate::adapters::database::message_repo::MessageRepository;
use crate::adapters::redis::IdempotencyRepository;
use crate::config::MessagingConfig;
use crate::domain::message::Message;
use crate::domain::notification::UserEvent;
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::{SendMessageResponse, send_message_request, send_message_response};
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
struct ParsedMessage {
    recipient_id: Uuid,
    submission_id: Uuid,
    msg_type: i32,
    content: Vec<u8>,
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

    /// Sends a batch of messages with idempotency support.
    ///
    /// # Errors
    /// Returns `AppError::Internal` if idempotency caching fails.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self, messages),
        fields(sender_id = %sender_id, count = messages.len())
    )]
    pub async fn send(
        &self,
        sender_id: Uuid,
        idempotency_key: Uuid,
        messages: Vec<send_message_request::Submission>,
    ) -> Result<SendMessageResponse> {
        // 1. Check Idempotency
        if let Ok(Some(cached)) = self.idempotency_repo.get_response(&idempotency_key.to_string()).await {
            tracing::info!(key = %idempotency_key, "Returning cached idempotency response");
            return SendMessageResponse::decode(cached.as_slice())
                .map_err(|e| AppError::InternalMsg(format!("Failed to decode cached idempotency response: {e}")));
        }

        if messages.is_empty() {
            return Ok(SendMessageResponse { failed_submissions: Vec::new() });
        }

        // 2. Parse and Validate Input
        let (parsed_batch, mut failed_submissions, recipient_ids_to_check) = Self::parse_incoming_batch(messages);

        // 3. Pre-Flight Check: Validate Recipients
        let mut tx = self.pool.begin().await?;
        let valid_recipients_set = if recipient_ids_to_check.is_empty() {
            std::collections::HashSet::new()
        } else {
            let ids: Vec<Uuid> = recipient_ids_to_check.into_iter().collect();
            self.repo.check_recipients_exist(&mut tx, &ids).await?.into_iter().collect()
        };

        // 4. Filter and Prepare Bulk Insert
        let mut valid_messages = Vec::with_capacity(parsed_batch.len());
        for parsed in parsed_batch {
            if valid_recipients_set.contains(&parsed.recipient_id) {
                valid_messages.push((parsed.recipient_id, parsed.submission_id, parsed.msg_type, parsed.content));
            } else {
                failed_submissions.push(send_message_response::FailedSubmission {
                    submission_id: parsed.submission_id.as_bytes().to_vec(),
                    error_code: send_message_response::ErrorCode::InvalidRecipient as i32,
                    error_message: "Recipient not found".to_string(),
                });
            }
        }

        // 5. Bulk Insert
        if !valid_messages.is_empty() {
            match self.repo.create_batch(&mut tx, sender_id, valid_messages, self.ttl_days).await {
                Ok(inserted) => {
                    tx.commit().await?;
                    self.metrics.sent_total.add(inserted.len() as u64, &[KeyValue::new("status", "success")]);

                    // Notify only for newly inserted messages
                    let recipient_ids: Vec<Uuid> = inserted.into_iter().map(|(id, _)| id).collect();
                    self.notifier.notify(&recipient_ids, UserEvent::MessageReceived).await;
                }
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to insert batch messages");
                    return Err(e);
                }
            }
        }

        let response = SendMessageResponse { failed_submissions };

        // 6. Cache Result
        let encoded = response.encode_to_vec();
        if let Err(e) = self
            .idempotency_repo
            .save_response(&idempotency_key.to_string(), &encoded, self.config.idempotency_ttl_secs)
            .await
        {
            tracing::error!(error = %e, "Failed to cache idempotency response");
        }

        Ok(response)
    }

    /// Internal helper to parse Protobuf messages into domain types.
    fn parse_incoming_batch(
        messages: Vec<send_message_request::Submission>,
    ) -> (Vec<ParsedMessage>, Vec<send_message_response::FailedSubmission>, std::collections::HashSet<Uuid>) {
        let mut failed_submissions = Vec::new();
        let mut parsed = Vec::with_capacity(messages.len());
        let mut recipient_ids = std::collections::HashSet::new();

        for outgoing in messages {
            let Ok(recipient_id) = Uuid::from_slice(&outgoing.recipient_id) else {
                failed_submissions.push(send_message_response::FailedSubmission {
                    submission_id: outgoing.submission_id,
                    error_code: send_message_response::ErrorCode::InvalidRecipient as i32,
                    error_message: "Invalid recipient UUID bytes (expected 16)".to_string(),
                });
                continue;
            };

            let Ok(submission_id) = Uuid::from_slice(&outgoing.submission_id) else {
                failed_submissions.push(send_message_response::FailedSubmission {
                    submission_id: outgoing.submission_id,
                    error_code: send_message_response::ErrorCode::Unspecified as i32,
                    error_message: "Invalid submission_id UUID bytes (expected 16)".to_string(),
                });
                continue;
            };

            let Some(msg) = outgoing.message else {
                failed_submissions.push(send_message_response::FailedSubmission {
                    submission_id: outgoing.submission_id,
                    error_code: send_message_response::ErrorCode::Unspecified as i32,
                    error_message: "Missing EncryptedMessage payload".to_string(),
                });
                continue;
            };

            parsed.push(ParsedMessage { recipient_id, submission_id, msg_type: msg.r#type, content: msg.content });
            recipient_ids.insert(recipient_id);
        }

        (parsed, failed_submissions, recipient_ids)
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
    pub(crate) const fn fetch_batch_limit(&self) -> i64 {
        self.config.fetch_batch_limit
    }
}
