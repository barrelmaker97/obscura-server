use crate::adapters::redis::RedisClient;
use crate::config::NotificationConfig;
use crate::domain::notification::UserEvent;
use crate::services::notification::NotificationService;
use crate::services::notification::provider::PushProvider;
use crate::services::notification::scheduler::NotificationScheduler;
use crate::services::notification::worker::NotificationWorker;
use crate::services::push_token_service::PushTokenService;
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
    pubsub: Arc<RedisClient>,
    scheduler: Arc<NotificationScheduler>,
    channels: Arc<DashMap<Uuid, broadcast::Sender<UserEvent>>>,
    channel_prefix: String,
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
        pubsub: Arc<RedisClient>,
        config: &NotificationConfig,
        shutdown: tokio::sync::watch::Receiver<bool>,
        provider: Option<Arc<dyn PushProvider>>,
        token_service: PushTokenService,
    ) -> anyhow::Result<Self> {
        let channels = Arc::new(DashMap::new());
        let metrics = Metrics::new();
        let scheduler = Arc::new(NotificationScheduler::new(Arc::clone(&pubsub), config.push_queue_key.clone()));

        let provider = provider.unwrap_or_else(|| Arc::new(crate::adapters::push::fcm::FcmPushProvider));

        let channel_prefix = config.channel_prefix.clone();
        let channel_pattern = format!("{channel_prefix}*");
        let push_delay_secs = config.push_delay_secs;

        tokio::spawn(
            Self::run_gc(Arc::clone(&channels), metrics.clone(), config.gc_interval_secs, shutdown.clone())
                .instrument(tracing::info_span!("notification_gc")),
        );

        // 2. Background Dispatcher task (PubSub -> Local Channels)
        let mut pubsub_rx = pubsub.subscribe(&channel_pattern).await?;
        let dispatcher_channels = Arc::clone(&channels);
        let dispatcher_metrics = metrics.clone();
        let mut dispatcher_shutdown = shutdown.clone();
        let dispatcher_prefix = channel_prefix.clone();

        tokio::spawn(
            async move {
                loop {
                    tokio::select! {
                        _ = dispatcher_shutdown.changed() => break,
                        msg = pubsub_rx.recv() => {
                            match msg {
                                Ok(msg) => {
                                    if let Some(user_id_str) = msg.channel.strip_prefix(&dispatcher_prefix)
                                        && let Ok(user_id) = Uuid::parse_str(user_id_str)
                                        && let Some(payload_byte) = msg.payload.first()
                                        && let Ok(event) = UserEvent::try_from(*payload_byte)
                                    {
                                        let event_label = format!("{event:?}");
                                        dispatcher_metrics.received_total.add(1, &[KeyValue::new("event", event_label.clone())]);
                                        if let Some(tx) = dispatcher_channels.get(&user_id) {
                                            let _ = tx.send(event);
                                        } else {
                                            dispatcher_metrics.unrouted_total.add(1, &[KeyValue::new("event", event_label)]);
                                        }
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    tracing::warn!(missed = n, "PubSub notification dispatcher lagged");
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("notification_dispatcher")),
        );

        // 3. Background Push Worker task
        let push_worker = NotificationWorker::new(
            Arc::clone(&scheduler),
            provider,
            token_service,
            config.worker_poll_limit,
            config.worker_interval_secs,
            config.worker_concurrency,
        );
        tokio::spawn(push_worker.run(shutdown).instrument(tracing::info_span!("push_worker")));

        Ok(Self {
            pubsub,
            scheduler,
            channels,
            channel_prefix,
            user_channel_capacity: config.user_channel_capacity,
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
        let channel_name = format!("{}{user_id}", self.channel_prefix);
        let payload = [event as u8];

        if let Err(e) = self.pubsub.publish(&channel_name, &payload).await {
            tracing::error!(error = %e, "Failed to publish to PubSub");
            self.metrics.sends_total.add(1, &[KeyValue::new("status", "error")]);
        } else {
            self.metrics.sends_total.add(1, &[KeyValue::new("status", "sent")]);
        }

        // Slow Path: Scheduled Push Fallback
        // Only trigger push for new messages for now.
        if event == UserEvent::MessageReceived {
            // Give the user some time to ACK via WebSocket before the push fires.
            if let Err(e) = self.scheduler.schedule_push(user_id, self.push_delay_secs).await {
                tracing::error!(error = %e, "Failed to schedule push notification");
            }
        }
    }

    #[tracing::instrument(skip(self), fields(user_id = %user_id))]
    async fn cancel_pending_notifications(&self, user_id: Uuid) {
        if let Err(e) = self.scheduler.cancel_push(user_id).await {
            tracing::error!(error = %e, "Failed to cancel pending push notification");
        }
    }
}
