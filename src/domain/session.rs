#[derive(Debug, Clone)]
pub struct Session {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}