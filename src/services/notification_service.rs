use crate::config::Config;
use async_trait::async_trait;
use dashmap::DashMap;
use opentelemetry::{
    global,
    metrics::{Counter, UpDownCounter},
    KeyValue,
};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct Metrics {
    sends_total: Counter<u64>,
    received_total: Counter<u64>,
    active_channels: UpDownCounter<i64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            sends_total: meter
                .u64_counter("notification_sends_total")
                .with_description("Total notification send attempts")
                .build(),
            received_total: meter
                .u64_counter("notification_received_total")
                .with_description("Total notifications received from Valkey")
                .build(),
            active_channels: meter
                .i64_up_down_counter("notification_active_channels")
                .with_description("Number of active local notification channels")
                .build(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UserEvent {
    MessageReceived = 1,
    Disconnect = 2,
}

impl TryFrom<u8> for UserEvent {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::MessageReceived),
            2 => Ok(Self::Disconnect),
            _ => Err(()),
        }
    }
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
    channels: Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
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
        let channels = Arc::new(DashMap::new());
        let metrics = Metrics::new();

        let pubsub_client = redis::Client::open(valkey_url)?;

        // Spawn background Pub/Sub listener
        tokio::spawn(
            Self::run_listener(pubsub_client, Arc::clone(&channels), metrics.clone(), shutdown.clone())
                .instrument(tracing::info_span!("notification_listener")),
        );

        // Spawn background GC task
        tokio::spawn(
            Self::run_gc(Arc::clone(&channels), metrics.clone(), config.notifications.gc_interval_secs, shutdown)
                .instrument(tracing::info_span!("notification_gc")),
        );

        Ok(Self { publisher, channels, channel_capacity: config.notifications.channel_capacity, metrics })
    }

    async fn run_listener(
        pubsub_client: redis::Client,
        channels: Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
        metrics: Metrics,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut backoff = std::time::Duration::from_secs(1);
        let max_backoff = std::time::Duration::from_secs(30);

        loop {
            let mut pubsub = match pubsub_client.get_async_pubsub().await {
                Ok(ps) => ps,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get async pubsub, retrying in {:?}", backoff);
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {
                            backoff = std::cmp::min(backoff * 2, max_backoff);
                            continue;
                        }
                        _ = shutdown.changed() => break,
                    }
                }
            };

            if let Err(e) = pubsub.psubscribe("user:*").await {
                tracing::error!(error = %e, "Failed to psubscribe, retrying in {:?}", backoff);
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {
                        backoff = std::cmp::min(backoff * 2, max_backoff);
                        continue;
                    }
                    _ = shutdown.changed() => break,
                }
            }

            tracing::info!("Successfully subscribed to Valkey notifications");
            backoff = std::time::Duration::from_secs(1); // Reset backoff on success

            let mut message_stream = pubsub.into_on_message();
            use futures::StreamExt;

            loop {
                tokio::select! {
                    _ = shutdown.changed() => return,

                    // Handle incoming messages from Valkey
                    msg = message_stream.next() => {
                        match msg {
                            Some(msg) => {
                                let channel = msg.get_channel_name();
                                if let Some(user_id_str) = channel.strip_prefix("user:") {
                                    if let Ok(user_id) = Uuid::parse_str(user_id_str) {
                                        let payload: u8 = msg.get_payload().unwrap_or_default();
                                        let event_res = UserEvent::try_from(payload);

                                        let span = tracing::debug_span!("process_notification", %user_id, ?event_res);
                                        let _enter = span.enter();

                                        if let Ok(event) = event_res {
                                            metrics.received_total.add(1, &[KeyValue::new("event", format!("{:?}", event))]);
                                            if let Some(tx) = channels.get(&user_id) {
                                                let _ = tx.send(event);
                                            }
                                        }
                                    }
                                }
                            }
                            None => {
                                tracing::warn!("Valkey pubsub connection lost, reconnecting...");
                                break; // Re-enter the outer loop to reconnect
                            }
                        }
                    }
                }
            }
        }
    }

    async fn run_gc(
        channels: Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
        metrics: Metrics,
        interval_secs: u64,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    channels.retain(|_, sender| {
                        let active = sender.receiver_count() > 0;
                        if !active {
                            metrics.active_channels.add(-1, &[]);
                        }
                        active
                    });
                }
                _ = shutdown.changed() => break,
            }
        }
    }
}

#[async_trait]
impl NotificationService for ValkeyNotificationService {
    #[tracing::instrument(skip(self), fields(user_id = %user_id))]
    async fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<UserEvent> {
        let tx = self
            .channels
            .entry(user_id)
            .or_insert_with(|| {
                self.metrics.active_channels.add(1, &[]);
                let (tx, _rx) = broadcast::channel(self.channel_capacity);
                tx
            })
            .value()
            .clone();

        tx.subscribe()
    }

    #[tracing::instrument(skip(self), fields(user_id = %user_id, event = ?event))]
    async fn notify(&self, user_id: Uuid, event: UserEvent) {
        use redis::AsyncCommands;
        let mut conn = self.publisher.clone();
        let channel_name = format!("user:{}", user_id);
        let payload = event as u8;

        match conn.publish::<_, _, i64>(channel_name, payload).await {
            Ok(_) => self.metrics.sends_total.add(1, &[KeyValue::new("status", "sent")]),
            Err(e) => {
                tracing::error!(error = %e, "Failed to publish to Valkey");
                self.metrics.sends_total.add(1, &[KeyValue::new("status", "error")]);
            }
        }
    }
}


