use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RegisterPushTokenRequest {
    pub token: String,
}

impl RegisterPushTokenRequest {
    /// Validates the token registration payload.
    ///
    /// # Errors
    /// Returns an error if the token is empty or excessively large (anti-abuse).
    pub fn validate(&self) -> Result<(), String> {
        let trimmed = self.token.trim();
        if trimmed.is_empty() {
            return Err("Token cannot be empty".into());
        }
        if trimmed.len() > 4096 {
            return Err("Token is too long (max 4096 characters)".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_token_success() {
        let req = RegisterPushTokenRequest { token: "valid_fcm_token_123".into() };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_validate_token_empty() {
        let req = RegisterPushTokenRequest { token: "   ".into() };
        let res = req.validate();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Token cannot be empty");
    }

    #[test]
    fn test_validate_token_too_long() {
        let req = RegisterPushTokenRequest { token: "A".repeat(4097) };
        let res = req.validate();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Token is too long (max 4096 characters)");
    }
}
