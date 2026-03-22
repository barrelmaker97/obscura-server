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
    pub device_id: Uuid,
    pub current_version: i32,
    pub pending_version: Option<i32>,
    pub state: BackupState,
    pub updated_at: OffsetDateTime,
    pub pending_at: Option<OffsetDateTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_state_from_str_active() {
        let state: BackupState = "ACTIVE".parse().expect("Valid state");
        assert_eq!(state, BackupState::Active);
    }

    #[test]
    fn test_backup_state_from_str_uploading() {
        let state: BackupState = "UPLOADING".parse().expect("Valid state");
        assert_eq!(state, BackupState::Uploading);
    }

    #[test]
    fn test_backup_state_from_str_invalid() {
        let result: Result<BackupState, _> = "INVALID".parse();
        assert!(result.is_err());
        assert!(result.expect_err("should fail for invalid state").contains("Invalid backup state"));
    }

    #[test]
    fn test_backup_state_display_roundtrip() {
        let active = BackupState::Active;
        let uploading = BackupState::Uploading;
        assert_eq!(active.to_string().parse::<BackupState>().expect("Active roundtrip"), active);
        assert_eq!(uploading.to_string().parse::<BackupState>().expect("Uploading roundtrip"), uploading);
    }
}
