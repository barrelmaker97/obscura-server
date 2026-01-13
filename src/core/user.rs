use serde::{Deserialize, Serialize};
use uuid::Uuid;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub created_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedPreKey {
    pub key_id: i32,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneTimePreKey {
    pub key_id: i32,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreKeyBundle {
    pub device_id: Uuid,
    pub registration_id: i32,
    pub identity_key: Vec<u8>,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_key: Option<OneTimePreKey>,
}
