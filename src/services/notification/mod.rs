use crate::domain::notification::UserEvent;
use async_trait::async_trait;
use tokio::sync::broadcast;
use uuid::Uuid;

pub mod distributed;
pub mod provider;
pub mod scheduler;
pub mod worker;

pub use distributed::DistributedNotificationService;

#[async_trait]
pub trait NotificationService: Send + Sync + std::fmt::Debug {
    // Returns a receiver that will get a value when a notification arrives.
    async fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<UserEvent>;

    // Sends a notification to the user.
    async fn notify(&self, user_id: Uuid, event: UserEvent);

    // Cancels any pending slow-path notifications (e.g. push).
    async fn cancel_pending_notifications(&self, user_id: Uuid);
}
