use crate::services::notification_service::NotificationService;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::watch;
use tracing::Instrument;

#[derive(Debug)]
pub struct NotificationWorker {
    service: NotificationService,
    gc_interval_secs: u64,
}

impl NotificationWorker {
    #[must_use]
    pub const fn new(service: NotificationService, gc_interval_secs: u64) -> Self {
        Self { service, gc_interval_secs }
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut gc_interval = tokio::time::interval(Duration::from_secs(self.gc_interval_secs));

        let mut notification_rx = match self.service.subscribe_realtime().await {
            Ok(rx) => rx,
            Err(e) => {
                tracing::error!(error = %e, "Failed to subscribe to real-time notifications, worker exiting");
                return;
            }
        };

        tracing::info!("Notification worker started");

        loop {
            tokio::select! {
                _ = shutdown.changed() => break,

                _ = gc_interval.tick() => {
                    async {
                        self.service.perform_gc();
                    }
                    .instrument(tracing::debug_span!("notification_gc_iteration"))
                    .await;
                }

                result = notification_rx.recv() => {
                    match result {
                        Ok(notification) => {
                            self.service.dispatch_event(&notification);
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(missed = n, "Internal notification dispatcher lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::error!("Notification stream closed, worker exiting");
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("Notification worker shutting down...");
    }
}
