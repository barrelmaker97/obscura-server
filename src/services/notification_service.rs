use crate::config::Config;
use crate::storage::valkey::ValkeyClient;
use async_trait::async_trait;
use dashmap::DashMap;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, UpDownCounter},
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

const CHANNEL_PREFIX: &str = "user:";
const CHANNEL_PATTERN: &str = "user:*";

#[derive(Debug)]
pub struct ValkeyNotificationService {
    valkey: Arc<ValkeyClient>,
    channels: Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
    channel_capacity: usize,
    metrics: Metrics,
}

impl ValkeyNotificationService {
    pub async fn new(
        valkey: Arc<ValkeyClient>,
        config: &Config,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<Self> {
        let channels = Arc::new(DashMap::new());
        let metrics = Metrics::new();

        // Background GC task
        tokio::spawn(
            Self::run_gc(
                Arc::clone(&channels),
                metrics.clone(),
                config.notifications.gc_interval_secs,
                shutdown.clone(),
            )
            .instrument(tracing::info_span!("notification_gc")),
        );

        // Background dispatcher task: subscribes to ValkeyClient and routes to local channels
        let mut valkey_rx = valkey.subscribe(CHANNEL_PATTERN).await?;
        let dispatcher_channels = Arc::clone(&channels);
        let dispatcher_metrics = metrics.clone();

        tokio::spawn(
            async move {
                let mut shutdown = shutdown;
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => break,
                        msg = valkey_rx.recv() => {
                            match msg {
                                Ok(msg) => {
                                    if let Some(user_id_str) = msg.channel.strip_prefix(CHANNEL_PREFIX) {
                                        if let Ok(user_id) = Uuid::parse_str(user_id_str) {
                                            if let Some(payload) = msg.payload.first() {
                                                if let Ok(event) = UserEvent::try_from(*payload) {
                                                    let span = tracing::debug_span!("process_notification", %user_id, ?event);
                                                    let _enter = span.enter();
                                                    dispatcher_metrics.received_total.add(1, &[KeyValue::new("event", format!("{:?}", event))]);
                                                    if let Some(tx) = dispatcher_channels.get(&user_id) {
                                                        let _ = tx.send(event);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    tracing::warn!(missed = n, "Valkey notification dispatcher lagged");
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("notification_dispatcher")),
        );

        Ok(Self { valkey, channels, channel_capacity: config.notifications.channel_capacity, metrics })
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
        let channel_name = format!("{}{}", CHANNEL_PREFIX, user_id);
        let payload = event as u8;

        match self.valkey.publish(&channel_name, payload).await {
            Ok(_) => self.metrics.sends_total.add(1, &[KeyValue::new("status", "sent")]),
            Err(e) => {
                tracing::error!(error = %e, "Failed to publish to Valkey");
                self.metrics.sends_total.add(1, &[KeyValue::new("status", "error")]);
            }
        }
    }
}
