use crate::adapters::redis::NotificationRepository;
use crate::config::NotificationConfig;
use crate::domain::notification::UserEvent;
use crate::services::notification::NotificationService;
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
                .with_description("Total notifications received from PubSub")
                .build(),
            unrouted_total: meter
                .u64_counter("notification_unrouted_total")
                .with_description("Notifications received from PubSub with no local subscribers")
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

#[derive(Debug)]
pub struct DistributedNotificationService {
    repo: Arc<NotificationRepository>,
    channels: Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
    user_channel_capacity: usize,
    push_delay_secs: u64,
    metrics: Metrics,
}

impl DistributedNotificationService {
    /// Creates a new distributed notification service.
    ///
    /// # Errors
    /// Returns an error if the subscription to `PubSub` fails.
    pub async fn new(
        repo: Arc<NotificationRepository>,
        config: &NotificationConfig,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<Self> {
        let channels = Arc::new(DashMap::new());
        let metrics = Metrics::new();

        let push_delay_secs = config.push_delay_secs;
        let user_channel_capacity = config.user_channel_capacity;

        tokio::spawn(
            Self::run_gc(Arc::clone(&channels), metrics.clone(), config.gc_interval_secs, shutdown.clone())
                .instrument(tracing::info_span!("notification_gc")),
        );

        // 2. Background Dispatcher task (PubSub -> Local Channels)
        let mut notification_rx = repo.subscribe_realtime().await?;
        let dispatcher_channels = Arc::clone(&channels);
        let dispatcher_metrics = metrics.clone();
        let mut dispatcher_shutdown = shutdown.clone();

        tokio::spawn(
            async move {
                loop {
                    tokio::select! {
                        _ = dispatcher_shutdown.changed() => break,
                        result = notification_rx.recv() => {
                            match result {
                                Ok(notification) => {
                                    let user_id = notification.user_id;
                                    let event = notification.event;
                                    let event_label = format!("{event:?}");

                                    dispatcher_metrics.received_total.add(1, &[KeyValue::new("event", event_label.clone())]);

                                    if let Some(tx) = dispatcher_channels.get(&user_id) {
                                        let _ = tx.send(event);
                                    } else {
                                        dispatcher_metrics.unrouted_total.add(1, &[KeyValue::new("event", event_label)]);
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    tracing::warn!(missed = n, "Internal notification dispatcher lagged");
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("notification_dispatcher")),
        );

        Ok(Self {
            repo,
            channels,
            user_channel_capacity,
            push_delay_secs,
            metrics,
        })
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
                    }
                }
                _ = shutdown.changed() => break,
            }
        }
    }
}

#[async_trait]
impl NotificationService for DistributedNotificationService {
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
        // Fast Path: WebSocket/PubSub
        if let Err(e) = self.repo.publish_realtime(user_id, event).await {
            tracing::error!(error = %e, "Failed to publish to PubSub");
            self.metrics.sends_total.add(1, &[KeyValue::new("status", "error")]);
        } else {
            self.metrics.sends_total.add(1, &[KeyValue::new("status", "sent")]);
        }

        // Slow Path: Scheduled Push Fallback
        // Only trigger push for new messages for now.
        if event == UserEvent::MessageReceived {
            // Give the user some time to ACK via WebSocket before the push fires.
            if let Err(e) = self.repo.push_job(user_id, self.push_delay_secs).await {
                tracing::error!(error = %e, "Failed to schedule push notification");
            }
        }
    }

    #[tracing::instrument(skip(self), fields(user_id = %user_id))]
    async fn cancel_pending_notifications(&self, user_id: Uuid) {
        if let Err(e) = self.repo.cancel_job(user_id).await {
            tracing::error!(error = %e, "Failed to cancel pending push notification");
        }
    }
}
