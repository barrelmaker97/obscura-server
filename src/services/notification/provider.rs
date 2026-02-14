use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait PushProvider: Send + Sync + std::fmt::Debug {
    async fn send_push(&self, user_id: Uuid) -> anyhow::Result<()>;
}

#[derive(Debug, Default)]
pub struct StubPushProvider;

#[async_trait]
impl PushProvider for StubPushProvider {
    async fn send_push(&self, user_id: Uuid) -> anyhow::Result<()> {
        tracing::info!(user_id = %user_id, "STUB: Sending push notification");
        Ok(())
    }
}
