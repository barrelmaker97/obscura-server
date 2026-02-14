use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RegisterTokenRequest {
    pub token: String,
}
