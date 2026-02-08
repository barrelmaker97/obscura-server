use crate::domain::crypto::{PublicKey, Signature};
use crate::domain::keys::{OneTimePreKey, SignedPreKey};
use time::OffsetDateTime;
use uuid::Uuid;

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
