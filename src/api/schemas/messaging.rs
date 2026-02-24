use crate::domain::message::{RawSubmission, SubmissionErrorCode, SubmissionOutcome};
use crate::proto::obscura::v1 as proto;

impl From<proto::send_message_request::Submission> for RawSubmission {
    fn from(proto: proto::send_message_request::Submission) -> Self {
        Self { submission_id: proto.submission_id, recipient_id: proto.recipient_id, message: proto.message }
    }
}

impl From<SubmissionOutcome> for proto::SendMessageResponse {
    fn from(outcome: SubmissionOutcome) -> Self {
        Self {
            failed_submissions: outcome
                .failed_submissions
                .into_iter()
                .map(|f| proto::send_message_response::FailedSubmission {
                    submission_id: f.submission_id,
                    error_code: match f.error_code {
                        SubmissionErrorCode::InvalidRecipient => {
                            proto::send_message_response::ErrorCode::InvalidRecipient
                        }
                        SubmissionErrorCode::MalformedRecipientId => {
                            proto::send_message_response::ErrorCode::MalformedRecipientId
                        }
                        SubmissionErrorCode::MalformedSubmissionId => {
                            proto::send_message_response::ErrorCode::MalformedSubmissionId
                        }
                        SubmissionErrorCode::MessageMissing => proto::send_message_response::ErrorCode::MessageMissing,
                    } as i32,
                    error_message: f.error_message,
                })
                .collect(),
        }
    }
}
