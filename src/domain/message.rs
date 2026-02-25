use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct Message {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub content: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
}

impl Message {}

#[derive(Debug, Clone)]
pub(crate) struct RawSubmission {
    pub submission_id: Vec<u8>,
    pub recipient_id: Vec<u8>,
    pub message: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SubmissionOutcome {
    pub failed_submissions: Vec<FailedSubmission>,
}

#[derive(Debug, Clone)]
pub struct FailedSubmission {
    pub submission_id: Vec<u8>, // Use raw bytes to preserve whatever the client sent
    pub error_code: SubmissionErrorCode,
    pub error_message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionErrorCode {
    InvalidRecipient,
    MalformedRecipientId,
    MalformedSubmissionId,
    MessageMissing,
}
