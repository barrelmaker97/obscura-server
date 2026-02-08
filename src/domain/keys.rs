use crate::domain::crypto::{PublicKey, Signature};
use crate::error::{AppError, Result};

#[derive(Debug, Clone)]
pub struct SignedPreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
    pub signature: Signature,
}

#[derive(Debug, Clone)]
pub struct OneTimePreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
}

impl OneTimePreKey {
}

#[derive(Debug, Clone)]
pub struct PreKeyBundle {
    pub registration_id: i32,
    pub identity_key: PublicKey,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_key: Option<OneTimePreKey>,
}

#[cfg(test)]
mod tests {
    use super::*;
}
