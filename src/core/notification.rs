use crate::config::Config;
use dashmap::DashMap;
use opentelemetry::{KeyValue, global, metrics::Counter};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone)]
struct NotificationMetrics {
    notification_sends_total: Counter<u64>,
}

impl NotificationMetrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            notification_sends_total: meter
                .u64_counter("notification_sends_total")
                .with_description("Total notification send attempts")
                .build(),
        }
    }
}

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
    metrics: NotificationMetrics,
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
                        let span = tracing::info_span!("notifier_gc_iteration");
                        let _enter = span.enter();
                        // Atomic cleanup: Remove entries with 0 receivers
                        map_ref.retain(|_, sender: &mut broadcast::Sender<UserEvent>| sender.receiver_count() > 0);
                    }
                    _ = shutdown.changed() => {}
                }
            }
        });

        Self {
            channels,
            channel_capacity: config.notifications.channel_capacity,
            metrics: NotificationMetrics::new(),
        }
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
            match tx.send(event) {
                Ok(_) => self.metrics.notification_sends_total.add(1, &[KeyValue::new("status", "sent")]),
                Err(_) => {
                    self.metrics.notification_sends_total.add(1, &[KeyValue::new("status", "no_receivers")])
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationConfig;
    use std::time::Duration;

    fn test_config(gc_interval: u64) -> Config {
        Config {
            notifications: NotificationConfig {
                gc_interval_secs: gc_interval,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_notifier_subscribe_and_notify() {
        let (_tx, rx_shutdown) = tokio::sync::watch::channel(false);
        let notifier = InMemoryNotifier::new(test_config(60), rx_shutdown);
        let user_id = Uuid::new_v4();

        let mut rx = notifier.subscribe(user_id);
        notifier.notify(user_id, UserEvent::MessageReceived);
        
        let event = rx.recv().await.unwrap();
        assert_eq!(event, UserEvent::MessageReceived);
    }

    #[tokio::test]
    async fn test_notifier_gc_logic() {
        let (_tx, rx_shutdown) = tokio::sync::watch::channel(false);
        let notifier = InMemoryNotifier::new(test_config(1), rx_shutdown);
        let user_id = Uuid::new_v4();

        // Subscribe and drop
        {
            let _rx = notifier.subscribe(user_id);
            assert_eq!(notifier.channels.len(), 1);
        }

        // Wait for GC
        tokio::time::sleep(Duration::from_millis(1500)).await;
        
        let mut success = false;
        for _ in 0..10 {
            if notifier.channels.is_empty() {
                success = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(success, "GC task did not clean up in time");
    }

    #[tokio::test]
    async fn test_notifier_gc_keeps_active() {
        let (_tx, rx_shutdown) = tokio::sync::watch::channel(false);
        let notifier = InMemoryNotifier::new(test_config(1), rx_shutdown);
        let user_id = Uuid::new_v4();

        let _rx = notifier.subscribe(user_id);
        
        tokio::time::sleep(Duration::from_millis(1500)).await;
        
        assert_eq!(notifier.channels.len(), 1);
    }

    #[tokio::test]
    async fn test_notifier_independent_channels() {
        let (_tx, rx_shutdown) = tokio::sync::watch::channel(false);
        let notifier = InMemoryNotifier::new(test_config(60), rx_shutdown);
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        let mut rx1 = notifier.subscribe(user1);
        let mut rx2 = notifier.subscribe(user2);
        
        notifier.notify(user1, UserEvent::MessageReceived);
        
        assert_eq!(rx1.recv().await.unwrap(), UserEvent::MessageReceived);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), rx2.recv()).await.is_err());
    }
}
