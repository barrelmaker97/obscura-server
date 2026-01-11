use async_trait::async_trait;
use tokio::sync::broadcast;
use uuid::Uuid;
use dashmap::DashMap;

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
    // Wrapped in Arc to share with background GC task.
    channels: std::sync::Arc<DashMap<Uuid, broadcast::Sender<()>>>,
}

impl InMemoryNotifier {
    pub fn new() -> Self {
        Self::new_with_interval(std::time::Duration::from_secs(60))
    }

    pub fn new_with_interval(cleanup_interval: std::time::Duration) -> Self {
        let channels = std::sync::Arc::new(DashMap::new());
        let map_ref = channels.clone();

        // Spawn background GC task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                interval.tick().await;
                // Atomic cleanup: Remove entries with 0 receivers
                map_ref.retain(|_, sender: &mut broadcast::Sender<()>| sender.receiver_count() > 0);
            }
        });

        Self {
            channels,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_notifier_gc_cleans_up_unused_channels() {
        // 1. Create notifier with fast cleanup (100ms)
        let notifier = InMemoryNotifier::new_with_interval(Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // 2. Subscribe (creates entry)
        let rx = notifier.subscribe(user_id);
        
        // Assert entry exists
        assert!(notifier.channels.contains_key(&user_id));
        assert_eq!(notifier.channels.len(), 1);

        // 3. Drop receiver (simulating disconnect)
        drop(rx);

        // 4. Wait for GC to run (wait 200ms to be safe)
        tokio::time::sleep(Duration::from_millis(200)).await;

        // 5. Assert entry is gone
        assert!(!notifier.channels.contains_key(&user_id));
        assert_eq!(notifier.channels.len(), 0);
    }
    
    #[tokio::test]
    async fn test_notifier_gc_keeps_active_channels() {
        let notifier = InMemoryNotifier::new_with_interval(Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // Subscribe and KEEP the receiver
        let _rx = notifier.subscribe(user_id);

        // Wait for GC
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Assert entry still exists
        assert!(notifier.channels.contains_key(&user_id));
    }
}