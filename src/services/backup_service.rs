use crate::adapters::database::DbPool;
use crate::adapters::database::backup_repo::BackupRepository;
use crate::adapters::storage::ObjectStorage;
use crate::config::StorageConfig;
use crate::domain::backup::BackupState;
use crate::error::{AppError, Result};
use aws_sdk_s3::primitives::ByteStream;
use axum::body::Body;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct BackupService {
    pool: DbPool,
    repo: BackupRepository,
    storage: Arc<dyn ObjectStorage>,
    config: StorageConfig,
}

impl std::fmt::Debug for BackupService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupService")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl BackupService {
    #[must_use]
    pub fn new(
        pool: DbPool,
        repo: BackupRepository,
        storage: Arc<dyn ObjectStorage>,
        config: StorageConfig,
    ) -> Self {
        Self { pool, repo, storage, config }
    }

    /// Handles the full backup upload workflow.
    ///
    /// # Errors
    /// Returns `AppError::PreconditionFailed` if `if_match_version` does not match.
    /// Returns `AppError::Conflict` if another upload is in progress.
    /// Returns `AppError::Timeout` if the upload takes too long.
    pub async fn handle_upload(
        &self, 
        user_id: Uuid, 
        if_match_version: i32, 
        content_len: Option<usize>,
        body: Body
    ) -> Result<()> {
        let mut conn = self.pool.acquire().await.map_err(AppError::Database)?;
        
        let _ = self.repo.create_if_not_exists(&mut conn, user_id).await?;
        
        let backup = if let Some(b) = self.repo.reserve_slot(&mut conn, user_id, if_match_version).await? {
            b
        } else {
            let current = self.repo.find_by_user_id(&mut conn, user_id).await?.ok_or(AppError::Internal)?;
            
            if current.current_version != if_match_version {
                return Err(AppError::PreconditionFailed);
            }
            
            if current.state == BackupState::Uploading {
                 let pending_at = current.pending_at.unwrap_or_else(OffsetDateTime::now_utc);
                 let threshold = OffsetDateTime::now_utc() - Duration::minutes(self.config.backup_stale_threshold_mins);
                 
                 if pending_at > threshold {
                     return Err(AppError::Conflict("Upload already in progress".into()));
                 }
                 
                 // Takeover
                 self.repo.reserve_slot_force(&mut conn, user_id).await?
            } else {
                return Err(AppError::Conflict("Concurrent modification".into()));
            }
        };
        
        drop(conn); 
        
        let pending_version = backup.pending_version.ok_or(AppError::Internal)?;
        let key = format!("{}{}/v{}", self.config.backup_prefix, user_id, pending_version);
        
        // Wrap storage put with timeout
        let put_future = self.storage.put(
            &key, 
            body, 
            content_len, 
            self.config.backup_max_size_bytes
        );

        match tokio::time::timeout(
            std::time::Duration::from_secs(self.config.backup_upload_timeout_secs),
            put_future
        ).await {
            Ok(Ok(())) => {},
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(AppError::Timeout),
        }
        
        let mut conn = self.pool.acquire().await.map_err(AppError::Database)?;
        self.repo.commit_version(&mut conn, user_id, pending_version).await?;
        
        // Cleanup old version
        let old_version = backup.current_version;
        if old_version > 0 {
             let old_key = format!("{}{}/v{}", self.config.backup_prefix, user_id, old_version);
             let storage = Arc::clone(&self.storage);
             tokio::spawn(async move {
                 let _ = storage.delete(&old_key).await;
             });
        }
        
        Ok(())
    }
    
    /// Downloads the current backup for the user.
    ///
    /// # Errors
    /// Returns `AppError::NotFound` if no backup exists.
    pub async fn download(&self, user_id: Uuid) -> Result<(i32, u64, ByteStream)> {
         let mut conn = self.pool.acquire().await.map_err(AppError::Database)?;
         let backup = self.repo.find_by_user_id(&mut conn, user_id).await?;
         
         if let Some(backup) = backup {
             if backup.current_version == 0 {
                 return Err(AppError::NotFound);
             }
             
             let key = format!("{}{}/v{}", self.config.backup_prefix, user_id, backup.current_version);
             let (len, stream) = self.storage.get(&key).await?;
             Ok((backup.current_version, len, stream))
         } else {
             Err(AppError::NotFound)
         }
    }
    
    /// Checks for the existence of a backup.
    ///
    /// # Errors
    /// Returns `AppError::NotFound` if no backup exists.
     pub async fn head(&self, user_id: Uuid) -> Result<(i32, u64)> {
         let mut conn = self.pool.acquire().await.map_err(AppError::Database)?;
         let backup = self.repo.find_by_user_id(&mut conn, user_id).await?;
         
         if let Some(backup) = backup {
             if backup.current_version == 0 {
                 return Err(AppError::NotFound);
             }
             
             let key = format!("{}{}/v{}", self.config.backup_prefix, user_id, backup.current_version);
             let len = self.storage.head(&key).await?;
             Ok((backup.current_version, len))
         } else {
             Err(AppError::NotFound)
         }
    }
}
