use uuid::Uuid;
use crate::error::Result;

#[derive(Clone, Debug, Default)]
pub struct PushTokenRepository {}

impl PushTokenRepository {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Finds all registered push tokens for a user.
    pub async fn find_tokens_for_user(&self, user_id: Uuid) -> Result<Vec<String>> {
        // STUB: This will eventually query the push_tokens table.
        // For testing, return a dummy token that encodes the user_id.
        Ok(vec![format!("token:{}", user_id)])
    }

    /// Deletes a specific push token (e.g. if invalidated by FCM).
    pub async fn delete_token(&self, _token: &str) -> Result<()> {
        // STUB: Implementation for deleting an invalid token.
        Ok(())
    }
}
