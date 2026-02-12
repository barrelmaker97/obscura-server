use crate::config::Config;
use crate::storage::valkey::ValkeyClient;
use async_trait::async_trait;
use dashmap::DashMap;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram, UpDownCounter},
};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct Metrics {
    sends_total: Counter<u64>,
    received_total: Counter<u64>,
    unrouted_total: Counter<u64>,
    active_channels: UpDownCounter<i64>,
    gc_duration_seconds: Histogram<f64>,
    gc_reclaimed_total: Counter<u64>,
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
            unrouted_total: meter
                .u64_counter("notification_unrouted_total")
                .with_description("Notifications received from Valkey with no local subscribers")
                .build(),
            active_channels: meter
                .i64_up_down_counter("notification_active_channels")
                .with_description("Number of active local notification channels")
                .build(),
            gc_duration_seconds: meter
                .f64_histogram("notification_gc_duration_seconds")
                .with_description("Time taken to perform a single GC iteration")
                .build(),
            gc_reclaimed_total: meter
                .u64_counter("notification_gc_reclaimed_total")
                .with_description("Total number of stale channels reclaimed by GC")
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
    user_channel_capacity: usize,
    metrics: Metrics,
}

impl ValkeyNotificationService {
    /// Creates a new Valkey notification service.
    ///
    /// # Errors
    /// Returns an error if the subscription to Valkey fails.
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
                                    if let Some(user_id_str) = msg.channel.strip_prefix(CHANNEL_PREFIX)
                                        && let Ok(user_id) = Uuid::parse_str(user_id_str)
                                        && let Some(payload_byte) = msg.payload.first() {
                                            if let Ok(event) = UserEvent::try_from(*payload_byte) {
                                                let span = tracing::debug_span!("process_notification", %user_id, ?event);
                                                let _enter = span.enter();
                                                dispatcher_metrics.received_total.add(1, &[KeyValue::new("event", format!("{event:?}"))]);
                                                if let Some(tx) = dispatcher_channels.get(&user_id) {
                                                    let _ = tx.send(event);
                                                } else {
                                                    dispatcher_metrics.unrouted_total.add(1, &[KeyValue::new("event", format!("{event:?}"))]);
                                                }
                                            } else {
                                                tracing::error!(payload = ?msg.payload, "Received invalid UserEvent payload");
                                            }
                                        }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    tracing::warn!(missed = n, "Valkey notification dispatcher lagged; missed {} notifications", n);
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("notification_dispatcher")),
        );

        Ok(Self { valkey, channels, user_channel_capacity: config.notifications.user_channel_capacity, metrics })
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
                    let start = std::time::Instant::now();
                    let mut reclaimed_this_cycle = 0;

                    let span = tracing::debug_span!("gc_iteration");
                    let _enter = span.enter();

                    channels.retain(|_, sender| {
                        let active = sender.receiver_count() > 0;
                        if !active {
                            metrics.active_channels.add(-1, &[]);
                            reclaimed_this_cycle += 1;
                        }
                        active
                    });

                    let duration = start.elapsed().as_secs_f64();
                    metrics.gc_duration_seconds.record(duration, &[]);
                    if reclaimed_this_cycle > 0 {
                        metrics.gc_reclaimed_total.add(reclaimed_this_cycle, &[]);
                        tracing::debug!(reclaimed = reclaimed_this_cycle, duration_secs = %duration, "GC reclaimed stale channels");
                    }
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
                let (tx, _rx) = broadcast::channel(self.user_channel_capacity);
                tx
            })
            .value()
            .clone();

        tx.subscribe()
    }

    #[tracing::instrument(skip(self), fields(user_id = %user_id, event = ?event))]
    async fn notify(&self, user_id: Uuid, event: UserEvent) {
        let channel_name = format!("{CHANNEL_PREFIX}{user_id}");
        let payload = [event as u8];

        match self.valkey.publish(&channel_name, &payload).await {
            Ok(()) => self.metrics.sends_total.add(1, &[KeyValue::new("status", "sent")]),
            Err(e) => {
                tracing::error!(error = %e, "Failed to publish to Valkey");
                self.metrics.sends_total.add(1, &[KeyValue::new("status", "error")]);
            }
        }
    }
}
