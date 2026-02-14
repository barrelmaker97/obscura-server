use crate::services::notification::scheduler::NotificationScheduler;
use crate::services::notification::provider::PushProvider;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

#[derive(Debug)]
pub struct NotificationWorker {
    scheduler: Arc<NotificationScheduler>,
    provider: Arc<dyn PushProvider>,
}

impl NotificationWorker {
    pub fn new(scheduler: Arc<NotificationScheduler>, provider: Arc<dyn PushProvider>) -> Self {
        Self { scheduler, provider }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        while !*shutdown.borrow() {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.process_due_jobs().await {
                        tracing::error!(error = %e, "Failed to process due notification jobs");
                    }
                }
                _ = shutdown.changed() => break,
            }
        }
        tracing::info!("Notification worker shutting down...");
    }

    #[tracing::instrument(skip(self), name = "process_due_jobs")]
    async fn process_due_jobs(&self) -> anyhow::Result<()> {
        // Pull a batch of due jobs. zpop_by_score is atomic.
        let user_ids = self.scheduler.pull_due_jobs(50).await?;

        if user_ids.is_empty() {
            return Ok(());
        }

        tracing::info!(count = user_ids.len(), "Processing due push notifications");

        for user_id in user_ids {
            let provider = Arc::clone(&self.provider);
            tokio::spawn(async move {
                if let Err(e) = provider.send_push(user_id).await {
                    tracing::error!(error = %e, user_id = %user_id, "Failed to send push notification");
                }
            }.instrument(tracing::debug_span!("send_push", user_id = %user_id)));
        }

        Ok(())
    }
}
