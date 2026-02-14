use crate::services::notification::scheduler::NotificationScheduler;
use crate::services::notification::provider::{PushProvider, PushError};
use crate::adapters::database::push_token_repo::PushTokenRepository;
use crate::adapters::database::DbPool;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

#[derive(Debug)]
pub struct NotificationWorker {
    pool: DbPool,
    scheduler: Arc<NotificationScheduler>,
    provider: Arc<dyn PushProvider>,
    token_repo: PushTokenRepository,
}

impl NotificationWorker {
    pub fn new(
        pool: DbPool,
        scheduler: Arc<NotificationScheduler>, 
        provider: Arc<dyn PushProvider>,
        token_repo: PushTokenRepository,
    ) -> Self {
        Self { pool, scheduler, provider, token_repo }
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

        // 1. Batch lookup tokens for all users in one query
        let mut conn = self.pool.acquire().await?;
        let user_token_pairs = self.token_repo.find_tokens_for_users(&mut conn, &user_ids).await?;

        // 2. Dispatch concurrently
        for (user_id, token) in user_token_pairs {
            let provider = Arc::clone(&self.provider);
            let token_repo = self.token_repo.clone();
            let pool = self.pool.clone();
            
            tokio::spawn(async move {
                match provider.send_push(&token).await {
                    Ok(()) => {
                        tracing::debug!(token = %token, "Push notification sent successfully");
                    }
                    Err(PushError::Unregistered) => {
                        tracing::info!(token = %token, "Token unregistered, deleting from database");
                        if let Ok(mut conn) = pool.acquire().await
                            && let Err(e) = token_repo.delete_token(&mut conn, &token).await
                        {
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
