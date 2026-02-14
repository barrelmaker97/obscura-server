use crate::services::notification::scheduler::NotificationScheduler;
use crate::services::notification::provider::PushProvider;
use crate::adapters::database::push_token_repo::PushTokenRepository;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

#[derive(Debug)]
pub struct NotificationWorker {
    scheduler: Arc<NotificationScheduler>,
    provider: Arc<dyn PushProvider>,
    token_repo: PushTokenRepository,
}

impl NotificationWorker {
    pub fn new(
        scheduler: Arc<NotificationScheduler>, 
        provider: Arc<dyn PushProvider>,
        token_repo: PushTokenRepository,
    ) -> Self {
        Self { scheduler, provider, token_repo }
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
        let user_ids = self.scheduler.pull_due_jobs(50).await?;

        if user_ids.is_empty() {
            return Ok(());
        }

        tracing::info!(count = user_ids.len(), "Processing due push notifications");

        for user_id in user_ids {
            let provider = Arc::clone(&self.provider);
            let token_repo = self.token_repo.clone();
            
            tokio::spawn(async move {
                // 1. Lookup tokens for the user
                let tokens = match token_repo.find_tokens_for_user(user_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!(error = %e, user_id = %user_id, "Failed to lookup push tokens");
                        return;
                    }
                };

                // 2. Dispatch to each token
                for token in tokens {
                    if let Err(e) = provider.send_push(&token).await {
                        // In the future, we can handle specific errors here (e.g. invalid token)
                        tracing::error!(error = %e, token = %token, "Failed to send push notification");
                    }
                }
            }.instrument(tracing::debug_span!("dispatch_push", user_id = %user_id)));
        }

        Ok(())
    }
}
