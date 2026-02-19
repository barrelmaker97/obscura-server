use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupState {
    Active,
    Uploading,
}

impl std::fmt::Display for BackupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "ACTIVE"),
            Self::Uploading => write!(f, "UPLOADING"),
        }
    }
}

impl std::str::FromStr for BackupState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "UPLOADING" => Ok(Self::Uploading),
            _ => Err(format!("Invalid backup state: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub user_id: Uuid,
    pub current_version: i32,
    pub pending_version: Option<i32>,
    pub state: BackupState,
    pub updated_at: OffsetDateTime,
    pub pending_at: Option<OffsetDateTime>,
}
