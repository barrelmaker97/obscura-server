use crate::config::Config;
use async_trait::async_trait;
use dashmap::DashMap;
use opentelemetry::{KeyValue, global, metrics::Counter};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct Metrics {
    sends_total: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            sends_total: meter
                .u64_counter("notification_sends_total")
                .with_description("Total notification send attempts")
                .build(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserEvent {
    MessageReceived,
    Disconnect,
}

#[async_trait]
pub trait NotificationService: Send + Sync + std::fmt::Debug {
    // Returns a receiver that will get a value when a notification arrives.
    async fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<UserEvent>;

    // Sends a notification to the user.
    async fn notify(&self, user_id: Uuid, event: UserEvent);
}

#[derive(Debug)]
pub struct ValkeyNotificationService {
    publisher: redis::aio::ConnectionManager,
    channels: std::sync::Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
    channel_capacity: usize,
    metrics: Metrics,
}

impl ValkeyNotificationService {
    pub async fn new(
        valkey_url: &str,
        config: &Config,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<Self> {
        let client = redis::Client::open(valkey_url)?;
        let publisher = client.get_connection_manager().await?;
        let channels = std::sync::Arc::new(DashMap::<Uuid, broadcast::Sender<UserEvent>>::new());

        let map_ref = std::sync::Arc::clone(&channels);
        let pubsub_client = redis::Client::open(valkey_url)?;
        let channel_capacity = config.notifications.channel_capacity;

        // Background Pub/Sub listener
        let mut listener_shutdown = shutdown.clone();
        tokio::spawn(async move {
            let mut pubsub = pubsub_client.get_async_pubsub().await.expect("Failed to get async pubsub");
            pubsub.psubscribe("user:*").await.expect("Failed to psubscribe to user:*");
            let mut message_stream = pubsub.into_on_message();
            use futures::StreamExt;

            loop {
                tokio::select! {
                    _ = listener_shutdown.changed() => break,

                    // Handle incoming messages from Valkey
                    Some(msg) = message_stream.next() => {
                        let channel = msg.get_channel_name();
                        if let Some(user_id_str) = channel.strip_prefix("user:") {
                            if let Ok(user_id) = Uuid::parse_str(user_id_str) {
                                let payload: String = msg.get_payload().unwrap_or_default();
                                let event = match payload.as_str() {
                                    "MessageReceived" => Some(UserEvent::MessageReceived),
                                    "Disconnect" => Some(UserEvent::Disconnect),
                                    _ => None,
                                };

                                if let Some(event) = event {
                                    if let Some(tx) = map_ref.get(&user_id) {
                                        let _ = tx.send(event);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        // Background GC task
        let gc_map = std::sync::Arc::clone(&channels);
        let gc_interval = config.notifications.gc_interval_secs;
        let mut gc_shutdown = shutdown.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(gc_interval));
            while !*gc_shutdown.borrow() {
                tokio::select! {
                    _ = interval.tick() => {
                        gc_map.retain(|_, sender: &mut broadcast::Sender<UserEvent>| sender.receiver_count() > 0);
                    }
                    _ = gc_shutdown.changed() => break,
                }
            }
        });

        Ok(Self { publisher, channels, channel_capacity, metrics: Metrics::new() })
    }
}

#[async_trait]
impl NotificationService for ValkeyNotificationService {
    async fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<UserEvent> {
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

    async fn notify(&self, user_id: Uuid, event: UserEvent) {
        use redis::AsyncCommands;
        let mut conn = self.publisher.clone();
        let channel_name = format!("user:{}", user_id);
        let payload = match event {
            UserEvent::MessageReceived => "MessageReceived",
            UserEvent::Disconnect => "Disconnect",
        };

        match conn.publish::<_, _, i64>(channel_name, payload).await {
            Ok(_) => self.metrics.sends_total.add(1, &[KeyValue::new("status", "sent")]),
            Err(e) => {
                tracing::error!(error = %e, user_id = %user_id, "Failed to publish to Valkey");
                self.metrics.sends_total.add(1, &[KeyValue::new("status", "error")]);
            }
        }
    }
}
