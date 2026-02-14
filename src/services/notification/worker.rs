use crate::services::notification::scheduler::NotificationScheduler;
use crate::services::notification::provider::{PushProvider, PushError};
use crate::services::push_token_service::PushTokenService;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

#[derive(Debug)]
pub struct NotificationWorker {
    scheduler: Arc<NotificationScheduler>,
    provider: Arc<dyn PushProvider>,
    token_service: PushTokenService,
    poll_limit: isize,
    interval_secs: u64,
    _concurrency: usize,
}

impl NotificationWorker {
    pub fn new(
        scheduler: Arc<NotificationScheduler>, 
        provider: Arc<dyn PushProvider>,
        token_service: PushTokenService,
        poll_limit: isize,
        interval_secs: u64,
        concurrency: usize,
    ) -> Self {
        Self { 
            scheduler, 
            provider, 
            token_service, 
            poll_limit, 
            interval_secs, 
            _concurrency: concurrency 
        }
    }

    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.interval_secs));

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
        let user_ids = self.scheduler.pull_due_jobs(self.poll_limit).await?;

        if user_ids.is_empty() {
            return Ok(());
        }

        tracing::info!(count = user_ids.len(), "Processing due push notifications");

        // 1. Batch lookup tokens for all users
        let user_token_pairs = self.token_service.get_tokens_for_users(&user_ids).await?;

        // 2. Dispatch concurrently
        for (user_id, token) in user_token_pairs {
            let provider = Arc::clone(&self.provider);
            let token_service = self.token_service.clone();
            
            tokio::spawn(async move {
                match provider.send_push(&token).await {
                    Ok(()) => {
                        tracing::debug!(token = %token, "Push notification sent successfully");
                    }
                    Err(PushError::Unregistered) => {
                        tracing::info!(token = %token, "Token unregistered, deleting from database");
                        if let Err(e) = token_service.invalidate_token(&token).await {
                            tracing::error!(error = %e, token = %token, "Failed to delete unregistered token");
                        }
                    }
                    Err(PushError::QuotaExceeded) => {
                        tracing::warn!("Push quota exceeded, should implement backoff");
                    }
                    Err(PushError::Other(e)) => {
                        tracing::error!(error = %e, token = %token, "Failed to send push notification");
                    }
                }
            }.instrument(tracing::debug_span!("dispatch_push", %user_id)));
        }

        Ok(())
    }
}
