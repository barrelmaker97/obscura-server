use crate::config::Config;
use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum UserEvent {
    MessageReceived,
    Disconnect,
}

#[async_trait]
pub trait Notifier: Send + Sync {
    // Returns a receiver that will get a value when a notification arrives.
    fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<UserEvent>;

    // Sends a notification to the user.
    fn notify(&self, user_id: Uuid, event: UserEvent);
}

pub struct InMemoryNotifier {
    // Map UserID -> Broadcast Channel
    // We store the Sender. We create new Receivers from it.
    // Wrapped in Arc to share with background GC task.
    channels: std::sync::Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
    channel_capacity: usize,
}

impl InMemoryNotifier {
    pub fn new(config: Config) -> Self {
        let channels = std::sync::Arc::new(DashMap::new());
        let map_ref = channels.clone();
        let interval_secs = config.notification_gc_interval_secs;

        // Spawn background GC task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                // Atomic cleanup: Remove entries with 0 receivers
                map_ref.retain(|_, sender: &mut broadcast::Sender<UserEvent>| sender.receiver_count() > 0);
            }
        });

        Self { channels, channel_capacity: config.notification_channel_capacity }
    }
}

#[async_trait]
impl Notifier for InMemoryNotifier {
    fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<UserEvent> {
        // Get existing channel or create new one
        let tx = self
            .channels
            .entry(user_id)
            .or_insert_with(|| {
                let (tx, _rx) = broadcast::channel(self.channel_capacity);
                tx
            })
            .value()
            .clone();

        tx.subscribe()
    }

    fn notify(&self, user_id: Uuid, event: UserEvent) {
        if let Some(tx) = self.channels.get(&user_id) {
            // We ignore errors (e.g., if no one is listening)
            let _ = tx.send(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Helper to create a dummy config for tests
    fn test_config(gc_interval: u64, capacity: usize) -> Config {
        Config {
            database_url: "".to_string(),
            jwt_secret: "".to_string(),
            rate_limit_per_second: 5,
            rate_limit_burst: 10,
            auth_rate_limit_per_second: 1,
            auth_rate_limit_burst: 3,
            server_host: "0.0.0.0".to_string(),
            server_port: 3000,
            message_ttl_days: 30,
            max_inbox_size: 1000,
            message_cleanup_interval_secs: 300,
            notification_gc_interval_secs: gc_interval,
            notification_channel_capacity: capacity,
            trusted_proxies: "127.0.0.1/32".to_string(),
        }
    }

    #[tokio::test]
    async fn test_notifier_gc_cleans_up_unused_channels() {
        let config = test_config(1, 16);
        let notifier = InMemoryNotifier::new(config);
        let user_id = Uuid::new_v4();

        // 2. Subscribe (creates entry)
        let rx = notifier.subscribe(user_id);

        // Assert entry exists
        assert!(notifier.channels.contains_key(&user_id));
        assert_eq!(notifier.channels.len(), 1);

        // 3. Drop receiver (simulating disconnect)
        drop(rx);

        // 4. Wait for GC to run (wait 1.1s to be safe)
        tokio::time::sleep(Duration::from_millis(1100)).await;

        // 5. Assert entry is gone
        assert!(!notifier.channels.contains_key(&user_id));
        assert_eq!(notifier.channels.len(), 0);
    }

    #[tokio::test]
    async fn test_notifier_gc_keeps_active_channels() {
        let config = test_config(1, 16);
        let notifier = InMemoryNotifier::new(config);
        let user_id = Uuid::new_v4();

        // Subscribe and KEEP the receiver
        let _rx = notifier.subscribe(user_id);

        // Wait for GC
        tokio::time::sleep(Duration::from_millis(1100)).await;

        // Assert entry still exists
        assert!(notifier.channels.contains_key(&user_id));
    }
}
