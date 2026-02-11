use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct IdentityKey {
    #[sqlx(rename = "user_id")]
    pub _user_id: Uuid,
    #[sqlx(rename = "identity_key")]
    pub key: Vec<u8>,
    pub registration_id: i32,
}

impl TryFrom<IdentityKey> for crate::domain::crypto::PublicKey {
    type Error = String;
    fn try_from(record: IdentityKey) -> Result<Self, Self::Error> {
        crate::domain::crypto::PublicKey::try_from_bytes(&record.key)
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct SignedPreKey {
    pub id: i32,
    #[sqlx(rename = "user_id")]
    pub _user_id: Uuid,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    #[sqlx(rename = "created_at")]
    pub _created_at: Option<OffsetDateTime>,
}

impl TryFrom<SignedPreKey> for crate::domain::keys::SignedPreKey {
    type Error = String;
    fn try_from(record: SignedPreKey) -> Result<Self, Self::Error> {
        let public_key = crate::domain::crypto::PublicKey::try_from(record.public_key)?;
        let signature = crate::domain::crypto::Signature::try_from(record.signature)?;
        Ok(crate::domain::keys::SignedPreKey { key_id: record.id, public_key, signature })
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct OneTimePreKey {
    pub id: i32,
    #[sqlx(rename = "user_id")]
    pub _user_id: Uuid,
    pub public_key: Vec<u8>,
    #[sqlx(rename = "created_at")]
    pub _created_at: Option<OffsetDateTime>,
}

impl TryFrom<OneTimePreKey> for crate::domain::keys::OneTimePreKey {
    type Error = String;
    fn try_from(record: OneTimePreKey) -> Result<Self, Self::Error> {
        let public_key = crate::domain::crypto::PublicKey::try_from(record.public_key)?;
        Ok(crate::domain::keys::OneTimePreKey { key_id: record.id, public_key })
    }
}
