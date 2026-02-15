#[derive(Debug, Clone)]
pub struct AuthSession {
    pub(crate) token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: i64,
}
