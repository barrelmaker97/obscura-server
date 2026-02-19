use crate::domain::backup::{Backup, BackupState};
use sqlx::FromRow;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub struct BackupRecord {
    pub user_id: Uuid,
    pub current_version: i32,
    pub pending_version: Option<i32>,
    pub state: String,
    pub updated_at: OffsetDateTime,
    pub pending_at: Option<OffsetDateTime>,
}

impl From<BackupRecord> for Backup {
    fn from(record: BackupRecord) -> Self {
        Self {
            user_id: record.user_id,
            current_version: record.current_version,
            pending_version: record.pending_version,
            state: BackupState::from_str(&record.state).unwrap_or(BackupState::Active),
            updated_at: record.updated_at,
            pending_at: record.pending_at,
        }
    }
}
