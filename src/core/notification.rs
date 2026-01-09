use async_trait::async_trait;
use tokio::sync::broadcast;
use uuid::Uuid;
use dashmap::DashMap;
use std::sync::Arc;

#[async_trait]
pub trait Notifier: Send + Sync {
    // Returns a receiver that will get a value when a notification arrives.
    fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<()>;
    
    // Sends a notification to the user.
    fn notify(&self, user_id: Uuid);
}

pub struct InMemoryNotifier {
    // Map UserID -> Broadcast Channel
    // We store the Sender. We create new Receivers from it.
    channels: DashMap<Uuid, broadcast::Sender<()>>,
}

impl InMemoryNotifier {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }
}

impl Default for InMemoryNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Notifier for InMemoryNotifier {
    fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<()> {
        // Get existing channel or create new one
        // broadcast channel capacity 16 is fine; we just need a "wake up" signal.
        let tx = self.channels
            .entry(user_id)
            .or_insert_with(|| {
                let (tx, _rx) = broadcast::channel(16);
                tx
            })
            .value()
            .clone();

        tx.subscribe()
    }

    fn notify(&self, user_id: Uuid) {
        if let Some(tx) = self.channels.get(&user_id) {
            // We ignore errors (e.g., if no one is listening)
            let _ = tx.send(());
        }
    }
}
