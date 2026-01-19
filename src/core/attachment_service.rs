use crate::config::Config;
use crate::storage::DbPool;
use aws_sdk_s3::Client;
use sqlx::Row;
use std::time::Duration;

pub struct AttachmentCleanupService {
    pool: DbPool,
    s3_client: Client,
    config: Config,
}

impl AttachmentCleanupService {
    pub fn new(pool: DbPool, s3_client: Client, config: Config) -> Self {
        Self { pool, s3_client, config }
    }

    pub async fn run_cleanup_loop(&self) {
        // Run every hour
        let mut interval = tokio::time::interval(Duration::from_secs(3600));

        loop {
            interval.tick().await;
            tracing::debug!("Running attachment cleanup...");

            if let Err(e) = self.cleanup_batch().await {
                tracing::error!("Attachment cleanup cycle failed: {:?}", e);
            }
        }
    }

    async fn cleanup_batch(&self) -> anyhow::Result<()> {
        loop {
            // Fetch expired attachments (Limit 100 per cycle to avoid blocking)
            let rows = sqlx::query("SELECT id FROM attachments WHERE expires_at < NOW() LIMIT 100")
                .fetch_all(&self.pool)
                .await?;

            if rows.is_empty() {
                break;
            }

            tracing::info!("Found {} expired attachments to delete", rows.len());

            for row in rows {
                let id: uuid::Uuid = row.get("id");
                let key = id.to_string();

                // 1. Delete from S3
                // We consider 'NotFound' or success as success. Network errors trigger retry next time.
                let res = self.s3_client.delete_object().bucket(&self.config.s3_bucket).key(&key).send().await;

                match res {
                    Ok(_) => {}
                    Err(aws_sdk_s3::error::SdkError::ServiceError(e)) => {
                        // Check if it's a 404 (NoSuchKey). If so, we can proceed to delete DB record.
                        // The SDK doesn't expose raw status easily without unwrapping, but delete_object is usually idempotent.
                        // If it's a real error, we log and skip DB delete.
                        tracing::warn!("S3 delete error for {}: {:?}", key, e);
                        // For safety, we skip DB delete if we aren't sure.
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("S3 network/transport error for {}: {:?}", key, e);
                        continue;
                    }
                }

                // 2. Delete from DB
                sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(&self.pool).await?;
            }
        }
        Ok(())
    }
}
