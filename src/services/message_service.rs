use crate::adapters::database::DbPool;
use crate::adapters::database::message_repo::MessageRepository;
use crate::config::MessagingConfig;
use crate::domain::message::{FailedSubmission, Message, RawSubmission, SubmissionErrorCode, SubmissionOutcome};
use crate::domain::notification::UserEvent;
use crate::error::Result;
use crate::services::notification_service::NotificationService;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
};
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
pub(crate) struct MessageService {
    pool: DbPool,
    repo: MessageRepository,
    notifier: NotificationService,
    ttl_days: i64,
    metrics: Metrics,
}

impl MessageService {
    #[must_use]
    pub(crate) fn new(
        pool: DbPool,
        repo: MessageRepository,
        notifier: NotificationService,
        _config: MessagingConfig,
        ttl_days: i64,
    ) -> Self {
        Self { pool, repo, notifier, ttl_days, metrics: Metrics::new() }
    }

    /// Processes a batch of raw submissions.
    /// Performs structural validation, recipient checking, and bulk insertion.
    ///
    /// # Errors
    /// Returns `AppError::Database` if any database operation fails.
    #[tracing::instrument(
        err(level = "warn"),
        skip(self, submissions),
        fields(sender_id = %sender_id, count = submissions.len())
    )]
    pub(crate) async fn send(&self, sender_id: Uuid, submissions: Vec<RawSubmission>) -> Result<SubmissionOutcome> {
        let mut failed_submissions = Vec::new();
        let mut potential_valid = Vec::with_capacity(submissions.len());
        let mut recipient_ids_to_check = std::collections::HashSet::new();

        // Pass 1: Structural Validation
        for raw in submissions {
            let Ok(submission_id) = Uuid::from_slice(&raw.submission_id) else {
                failed_submissions.push(FailedSubmission {
                    submission_id: raw.submission_id,
                    error_code: SubmissionErrorCode::MalformedSubmissionId,
                    error_message: "Invalid submission_id UUID bytes (expected 16)".to_string(),
                });
                continue;
            };

            let Ok(recipient_id) = Uuid::from_slice(&raw.recipient_id) else {
                failed_submissions.push(FailedSubmission {
                    submission_id: raw.submission_id,
                    error_code: SubmissionErrorCode::MalformedRecipientId,
                    error_message: "Invalid recipient UUID bytes (expected 16)".to_string(),
                });
                continue;
            };

            if raw.message.is_empty() {
                failed_submissions.push(FailedSubmission {
                    submission_id: raw.submission_id,
                    error_code: SubmissionErrorCode::MessageMissing,
                    error_message: "Missing message payload".to_string(),
                });
                continue;
            }

            recipient_ids_to_check.insert(recipient_id);
            potential_valid.push((recipient_id, submission_id, raw.message));
        }

        if potential_valid.is_empty() {
            return Ok(SubmissionOutcome { failed_submissions });
        }

        // Pass 2: Business Validation (Recipient Existence)
        let mut tx = self.pool.begin().await?;
        let check_ids: Vec<Uuid> = recipient_ids_to_check.into_iter().collect();
        let valid_recipients_set: std::collections::HashSet<Uuid> =
            self.repo.check_recipients_exist(&mut tx, &check_ids).await?.into_iter().collect();

        let mut to_insert = Vec::with_capacity(potential_valid.len());
        for (r_id, s_id, msg) in potential_valid {
            if valid_recipients_set.contains(&r_id) {
                to_insert.push((r_id, s_id, msg));
            } else {
                failed_submissions.push(FailedSubmission {
                    submission_id: s_id.as_bytes().to_vec(),
                    error_code: SubmissionErrorCode::InvalidRecipient,
                    error_message: "Recipient not found".to_string(),
                });
            }
        }

        // Pass 3: Bulk Insert
        if !to_insert.is_empty() {
            let inserted = self.repo.create_batch(&mut tx, sender_id, to_insert, self.ttl_days).await?;
            tx.commit().await?;

            self.metrics.sent_total.add(inserted.len() as u64, &[KeyValue::new("status", "success")]);

            // Notify Recipients
            let inserted_recipient_ids: Vec<Uuid> = inserted.into_iter().map(|(id, _)| id).collect();
            self.notifier.notify(&inserted_recipient_ids, UserEvent::MessageReceived).await;
        }

        Ok(SubmissionOutcome { failed_submissions })
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
    pub(crate) async fn fetch_pending_batch(
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
    pub(crate) async fn delete_batch(&self, user_id: Uuid, message_ids: &[Uuid]) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        self.repo.delete_batch(&mut conn, user_id, message_ids).await
    }
}
