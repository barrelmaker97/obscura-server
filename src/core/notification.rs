use crate::config::Config;
use dashmap::DashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum UserEvent {
    MessageReceived,
    Disconnect,
}

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
    pub fn new(config: Config, mut shutdown: tokio::sync::watch::Receiver<bool>) -> Self {
        let channels = std::sync::Arc::new(DashMap::new());
        let map_ref = channels.clone();
        let interval_secs = config.notifications.gc_interval_secs;

        // Spawn background GC task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            while !*shutdown.borrow() {
                tokio::select! {
                    _ = interval.tick() => {
                        // Atomic cleanup: Remove entries with 0 receivers
                        map_ref.retain(|_, sender: &mut broadcast::Sender<UserEvent>| sender.receiver_count() > 0);
                    }
                    _ = shutdown.changed() => {}
                }
            }
        });

        Self { channels, channel_capacity: config.notifications.channel_capacity }
    }
}

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
            ttl_days: 30,
            server: crate::config::ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 3000,
                mgmt_port: 9000,
                trusted_proxies: vec!["127.0.0.1/32".parse().unwrap()],
            },
            auth: crate::config::AuthConfig {
                jwt_secret: "".to_string(),
                access_token_ttl_secs: 900,
                refresh_token_ttl_days: 30,
            },
            rate_limit: crate::config::RateLimitConfig { per_second: 5, burst: 10, auth_per_second: 1, auth_burst: 3 },
            health: crate::config::HealthConfig { db_timeout_ms: 2000, s3_timeout_ms: 2000 },
            messaging: crate::config::MessagingConfig {
                max_inbox_size: 1000,
                cleanup_interval_secs: 300,
                batch_limit: 50,
                pre_key_refill_threshold: 20,
                max_pre_keys: 100,
            },
            notifications: crate::config::NotificationConfig {
                gc_interval_secs: gc_interval,
                channel_capacity: capacity,
            },
            websocket: crate::config::WsConfig {
                outbound_buffer_size: 32,
                ack_buffer_size: 100,
                ack_batch_size: 50,
                ack_flush_interval_ms: 500,
            },
            s3: crate::config::S3Config {
                bucket: "".to_string(),
                region: "us-east-1".to_string(),
                endpoint: None,
                access_key: None,
                secret_key: None,
                force_path_style: false,
                attachment_max_size_bytes: 52_428_800,
            },
        }
    }

    #[tokio::test]
    async fn test_notifier_gc_cleans_up_unused_channels() {
        let config = test_config(1, 16);
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let notifier = InMemoryNotifier::new(config, rx);
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
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let notifier = InMemoryNotifier::new(config, rx);
        let user_id = Uuid::new_v4();

        // Subscribe and KEEP the receiver
        let _rx = notifier.subscribe(user_id);

        // Wait for GC
        tokio::time::sleep(Duration::from_millis(1100)).await;

        // Assert entry still exists
        assert!(notifier.channels.contains_key(&user_id));
    }
}
