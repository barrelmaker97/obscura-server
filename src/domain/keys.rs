use crate::domain::crypto::{PublicKey, Signature};

#[derive(Debug, Clone)]
pub struct SignedPreKey {
    pub(crate) key_id: i32,
    pub(crate) public_key: PublicKey,
    pub(crate) signature: Signature,
}

#[derive(Debug, Clone)]
pub struct OneTimePreKey {
    pub(crate) key_id: i32,
    pub(crate) public_key: PublicKey,
}

#[derive(Debug, Clone)]
pub struct PreKeyBundle {
    pub(crate) registration_id: i32,
    pub(crate) identity_key: PublicKey,
    pub(crate) signed_pre_key: SignedPreKey,
    pub(crate) one_time_pre_key: Option<OneTimePreKey>,
}
