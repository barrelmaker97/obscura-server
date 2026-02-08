use crate::domain::attachment::Attachment;
use crate::domain::auth::RefreshToken;
use crate::domain::crypto::{PublicKey, Signature};
use crate::domain::keys::{OneTimePreKey, SignedPreKey};
use crate::domain::message::Message;
use crate::domain::user::User;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct UserRecord {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub created_at: Option<OffsetDateTime>,
}

impl From<UserRecord> for User {
    fn from(record: UserRecord) -> Self {
        Self {
            id: record.id,
            username: record.username,
            password_hash: record.password_hash,
            created_at: record.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct MessageRecord {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub recipient_id: Uuid,
    pub message_type: i32,
    pub content: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
}

impl From<MessageRecord> for Message {
    fn from(record: MessageRecord) -> Self {
        Self {
            id: record.id,
            sender_id: record.sender_id,
            recipient_id: record.recipient_id,
            message_type: record.message_type,
            content: record.content,
            created_at: record.created_at,
            expires_at: record.expires_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct RefreshTokenRecord {
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

impl From<RefreshTokenRecord> for RefreshToken {
    fn from(record: RefreshTokenRecord) -> Self {
        Self {
            token_hash: record.token_hash,
            user_id: record.user_id,
            expires_at: record.expires_at,
            created_at: record.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct AttachmentRecord {
    pub id: Uuid,
    pub expires_at: OffsetDateTime,
}

impl From<AttachmentRecord> for Attachment {
    fn from(record: AttachmentRecord) -> Self {
        Self {
            id: record.id,
            expires_at: record.expires_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct IdentityKeyRecord {
    pub user_id: Uuid,
    pub identity_key: Vec<u8>,
    pub registration_id: i32,
}

impl TryFrom<IdentityKeyRecord> for PublicKey {
    type Error = String;
    fn try_from(record: IdentityKeyRecord) -> Result<Self, Self::Error> {
        PublicKey::try_from(record.identity_key)
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct SignedPreKeyRecord {
    pub id: i32,
    pub user_id: Uuid,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
}

impl TryFrom<SignedPreKeyRecord> for SignedPreKey {
    type Error = String;
    fn try_from(record: SignedPreKeyRecord) -> Result<Self, Self::Error> {
        let public_key = PublicKey::try_from(record.public_key)?;
        let signature = Signature::try_from(record.signature)?;
        Ok(SignedPreKey {
            key_id: record.id,
            public_key,
            signature,
        })
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct OneTimePreKeyRecord {
    pub id: i32,
    pub user_id: Uuid,
    pub public_key: Vec<u8>,
    pub created_at: Option<OffsetDateTime>,
}

impl TryFrom<OneTimePreKeyRecord> for OneTimePreKey {
    type Error = String;
    fn try_from(record: OneTimePreKeyRecord) -> Result<Self, Self::Error> {
        let public_key = PublicKey::try_from(record.public_key)?;
        Ok(OneTimePreKey {
            key_id: record.id,
            public_key,
        })
    }
}
