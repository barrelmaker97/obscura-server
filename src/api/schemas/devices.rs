use crate::api::schemas::crypto::PublicKey;
use crate::api::schemas::keys::{OneTimePreKey, SignedPreKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeviceRequest {
    pub name: Option<String>,
    pub identity_key: PublicKey,
    pub registration_id: i32,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_keys: Vec<OneTimePreKey>,
}

impl CreateDeviceRequest {
    /// Validates the device creation payload.
    ///
    /// # Errors
    /// Returns an error if there are duplicate pre-key IDs.
    pub fn validate(&self) -> Result<(), String> {
        let mut unique_ids = std::collections::HashSet::with_capacity(self.one_time_pre_keys.len());
        for pk in &self.one_time_pre_keys {
            if !unique_ids.insert(pk.key_id) {
                return Err(format!("Duplicate prekey ID: {}", pk.key_id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceResponse {
    pub device_id: String,
    pub name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeviceRequest {
    pub name: Option<String>,
}
