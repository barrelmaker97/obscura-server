use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentResponse {
    pub id: Uuid,
    pub expires_at: i64,
}
