use crate::adapters::push::{PushError, PushProvider};
use crate::config::FcmConfig;
use async_trait::async_trait;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// How many seconds before expiry to proactively refresh the token.
const REFRESH_MARGIN_SECS: u64 = 300;

/// Google `OAuth2` scope required for FCM.
const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// Google token endpoint.
const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Grant type for JWT bearer assertion (RFC 7523).
const JWT_BEARER_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// Fields parsed from a Google service account JSON file.
#[derive(Debug, Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
}

/// JWT claims for the Google `OAuth2` token exchange.
#[derive(Debug, Serialize)]
struct Claims {
    iss: String,
    scope: String,
    aud: String,
    iat: u64,
    exp: u64,
}

/// Response from Google's token endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// A cached `OAuth2` access token with its expiry.
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    /// Absolute time (seconds since UNIX epoch) when this token expires.
    expires_at: u64,
}

impl CachedToken {
    /// Returns `true` if the token should be refreshed (within the refresh margin).
    fn needs_refresh(&self) -> bool {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        now + REFRESH_MARGIN_SECS >= self.expires_at
    }
}

/// FCM HTTP v1 push notification provider.
///
/// Uses a Google service account to acquire `OAuth2` access tokens via the JWT
/// bearer assertion flow (RFC 7523), caches them with proactive refresh, and
/// sends data-only messages to the FCM API.
#[derive(Debug)]
pub struct FcmPushProvider {
    project_id: String,
    client_email: String,
    encoding_key: EncodingKey,
    http: reqwest::Client,
    token_cache: Arc<RwLock<Option<CachedToken>>>,
}

/// FCM error detail returned in the response body.
#[derive(Debug, Deserialize)]
struct FcmErrorResponse {
    error: Option<FcmErrorBody>,
}

#[derive(Debug, Deserialize)]
struct FcmErrorBody {
    status: Option<String>,
    details: Option<Vec<FcmErrorDetail>>,
}

#[derive(Debug, Deserialize)]
struct FcmErrorDetail {
    #[serde(rename = "errorCode")]
    error_code: Option<String>,
}

/// FCM message payload.
#[derive(Debug, Serialize)]
struct FcmRequest {
    message: FcmMessage,
}

#[derive(Debug, Serialize)]
struct FcmMessage {
    token: String,
    data: FcmData,
    android: FcmAndroid,
}

#[derive(Debug, Serialize)]
struct FcmData {
    action: String,
}

#[derive(Debug, Serialize)]
struct FcmAndroid {
    collapse_key: String,
    priority: String,
}

impl FcmPushProvider {
    /// Creates a new `FcmPushProvider` by reading the service account key from a file.
    ///
    /// # Errors
    /// Returns an error if the credentials file cannot be read or parsed.
    pub fn new(config: &FcmConfig) -> Result<Self, anyhow::Error> {
        let project_id = config
            .project_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("FCM project ID is required (set OBSCURA_FCM_PROJECT_ID)"))?;

        let credentials_file = config
            .credentials_file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("FCM credentials file is required (set OBSCURA_FCM_CREDENTIALS_FILE)"))?;

        let key_json = std::fs::read_to_string(credentials_file)
            .map_err(|e| anyhow::anyhow!("Failed to read FCM credentials file '{credentials_file}': {e}"))?;

        let sa_key: ServiceAccountKey = serde_json::from_str(&key_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse FCM service account JSON: {e}"))?;

        let encoding_key = EncodingKey::from_rsa_pem(sa_key.private_key.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to parse RSA private key from service account: {e}"))?;

        Ok(Self {
            project_id: project_id.clone(),
            client_email: sa_key.client_email,
            encoding_key,
            http: reqwest::Client::new(),
            token_cache: Arc::new(RwLock::new(None)),
        })
    }

    /// Returns a valid `OAuth2` access token, refreshing if necessary.
    ///
    /// Uses a read-biased caching strategy: most callers take a read lock and
    /// get the cached token. Only when the token is missing or near expiry does
    /// a single caller acquire a write lock and refresh.
    async fn get_access_token(&self) -> Result<String, PushError> {
        // Fast path: read lock
        {
            let cache = self.token_cache.read().await;
            if let Some(ref cached) = *cache
                && !cached.needs_refresh()
            {
                return Ok(cached.access_token.clone());
            }
        }

        // Slow path: write lock to refresh
        let mut cache = self.token_cache.write().await;

        // Double-check after acquiring write lock (another task may have refreshed)
        if let Some(ref cached) = *cache
            && !cached.needs_refresh()
        {
            return Ok(cached.access_token.clone());
        }

        let token = self.fetch_access_token().await?;
        let access_token = token.access_token.clone();
        *cache = Some(token);
        drop(cache);
        Ok(access_token)
    }

    /// Performs the JWT bearer assertion flow to obtain a new access token.
    async fn fetch_access_token(&self) -> Result<CachedToken, PushError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        let claims = Claims {
            iss: self.client_email.clone(),
            scope: FCM_SCOPE.to_string(),
            aud: GOOGLE_TOKEN_URI.to_string(),
            iat: now,
            exp: now + 3600,
        };

        let header = Header::new(Algorithm::RS256);
        let assertion = jsonwebtoken::encode(&header, &claims, &self.encoding_key)
            .map_err(|e| PushError::Other(anyhow::anyhow!("Failed to encode JWT assertion: {e}")))?;

        let resp = self
            .http
            .post(GOOGLE_TOKEN_URI)
            .form(&[("grant_type", JWT_BEARER_GRANT_TYPE), ("assertion", &assertion)])
            .send()
            .await
            .map_err(|e| PushError::Other(anyhow::anyhow!("Token exchange request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(PushError::Other(anyhow::anyhow!("Token exchange failed with HTTP {status}: {body}")));
        }

        let token_resp: TokenResponse =
            resp.json().await.map_err(|e| PushError::Other(anyhow::anyhow!("Failed to parse token response: {e}")))?;

        Ok(CachedToken { access_token: token_resp.access_token, expires_at: now + token_resp.expires_in })
    }

    /// Sends a data-only push notification via the FCM HTTP v1 API.
    async fn send_fcm_message(&self, device_token: &str) -> Result<(), PushError> {
        let access_token = self.get_access_token().await?;

        let url = format!("https://fcm.googleapis.com/v1/projects/{}/messages:send", self.project_id);

        let body = FcmRequest {
            message: FcmMessage {
                token: device_token.to_string(),
                data: FcmData { action: "check".to_string() },
                android: FcmAndroid { collapse_key: "obscura_check".to_string(), priority: "high".to_string() },
            },
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| PushError::Other(anyhow::anyhow!("FCM request failed: {e}")))?;

        let status = resp.status();

        if status.is_success() {
            return Ok(());
        }

        // Map HTTP status codes and FCM error codes to PushError variants
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(PushError::QuotaExceeded);
        }

        let body = resp.text().await.unwrap_or_default();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(PushError::Unregistered);
        }

        // Parse the error body for specific FCM error codes
        if let Ok(error_resp) = serde_json::from_str::<FcmErrorResponse>(&body)
            && let Some(ref error) = error_resp.error
        {
            // Check top-level status
            if let Some(ref s) = error.status
                && (s == "NOT_FOUND" || s == "UNREGISTERED")
            {
                return Err(PushError::Unregistered);
            }

            // Check error details for UNREGISTERED error code
            if let Some(ref details) = error.details {
                for detail in details {
                    if let Some(ref code) = detail.error_code
                        && code == "UNREGISTERED"
                    {
                        return Err(PushError::Unregistered);
                    }
                }
            }
        }

        Err(PushError::Other(anyhow::anyhow!("FCM request failed with HTTP {status}: {body}")))
    }
}

#[async_trait]
impl PushProvider for FcmPushProvider {
    async fn send_push(&self, token: &str) -> Result<(), PushError> {
        self.send_fcm_message(token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_token_needs_refresh_when_expired() {
        let token = CachedToken { access_token: "test".to_string(), expires_at: 0 };
        assert!(token.needs_refresh());
    }

    #[test]
    fn cached_token_does_not_need_refresh_when_fresh() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let token = CachedToken { access_token: "test".to_string(), expires_at: now + 3600 };
        assert!(!token.needs_refresh());
    }

    #[test]
    fn cached_token_needs_refresh_within_margin() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let token = CachedToken { access_token: "test".to_string(), expires_at: now + REFRESH_MARGIN_SECS - 1 };
        assert!(token.needs_refresh());
    }

    #[tokio::test]
    async fn get_access_token_returns_cached_value() {
        let http = reqwest::Client::new();
        let provider = FcmPushProvider {
            project_id: "test-project".to_string(),
            client_email: "test@test.iam.gserviceaccount.com".to_string(),
            encoding_key: EncodingKey::from_secret(b"unused"),
            http,
            token_cache: Arc::new(RwLock::new(None)),
        };

        // Pre-populate the cache
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        {
            let mut cache = provider.token_cache.write().await;
            *cache = Some(CachedToken { access_token: "cached_token".to_string(), expires_at: now + 3600 });
        }

        // Multiple calls should return the cached token without hitting any server
        let t1 = provider.get_access_token().await.unwrap();
        let t2 = provider.get_access_token().await.unwrap();
        assert_eq!(t1, "cached_token");
        assert_eq!(t2, "cached_token");
    }

    #[test]
    fn parse_fcm_error_with_unregistered_detail() {
        let body = r#"{"error":{"status":"INVALID_ARGUMENT","details":[{"errorCode":"UNREGISTERED"}]}}"#;
        let error_resp: FcmErrorResponse = serde_json::from_str(body).unwrap();
        let error = error_resp.error.unwrap();
        let details = error.details.unwrap();
        assert_eq!(details[0].error_code.as_deref(), Some("UNREGISTERED"));
    }

    #[test]
    fn parse_fcm_error_with_not_found_status() {
        let body = r#"{"error":{"status":"NOT_FOUND"}}"#;
        let error_resp: FcmErrorResponse = serde_json::from_str(body).unwrap();
        let error = error_resp.error.unwrap();
        assert_eq!(error.status.as_deref(), Some("NOT_FOUND"));
    }

    #[test]
    fn parse_fcm_error_with_no_details() {
        let body = r#"{"error":{"status":"INTERNAL"}}"#;
        let error_resp: FcmErrorResponse = serde_json::from_str(body).unwrap();
        let error = error_resp.error.unwrap();
        assert_eq!(error.status.as_deref(), Some("INTERNAL"));
        assert!(error.details.is_none());
    }

    #[test]
    fn parse_service_account_key() {
        let json = r#"{"client_email":"test@test.iam.gserviceaccount.com","private_key":"-----BEGIN RSA PRIVATE KEY-----\ntest\n-----END RSA PRIVATE KEY-----\n"}"#;
        let key: ServiceAccountKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.client_email, "test@test.iam.gserviceaccount.com");
        assert!(key.private_key.contains("RSA PRIVATE KEY"));
    }
}
