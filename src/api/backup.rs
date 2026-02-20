use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::{AppError, Result};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;

/// Uploads a new backup version.
///
/// # Errors
/// Returns `AppError::BadRequest` if headers are invalid.
/// Returns `AppError::LengthRequired` if the Content-Length header is missing.
/// Returns `AppError::Internal` if the upload fails.
pub async fn upload_backup(
    auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse> {
    // 1. Determine target version using Optimistic Locking headers
    let if_match_version = if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH) {
        if if_none_match == "*" {
            0 // Standard way to say "only if it doesn't exist"
        } else {
            return Err(AppError::BadRequest("Invalid If-None-Match header".into()));
        }
    } else {
        let if_match_header = headers
            .get(header::IF_MATCH)
            .ok_or(AppError::BadRequest("Missing If-Match or If-None-Match header".into()))?
            .to_str()
            .map_err(|_| AppError::BadRequest("Invalid If-Match header".into()))?;

        let if_match_str = if_match_header.trim_matches('"');
        if_match_str.parse::<i32>().map_err(|_| AppError::BadRequest("Invalid version in If-Match header".into()))?
    };

    let content_len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<usize>().ok()))
        .ok_or(AppError::LengthRequired)?;

    // Bridge Axum Body -> StorageStream (using neutral std::io::Error)
    let stream = body.into_data_stream().map(|res| res.map_err(|e| std::io::Error::other(e.to_string()))).boxed();

    state.backup_service.handle_upload(auth_user.user_id, if_match_version, Some(content_len), stream).await?;

    Ok(StatusCode::OK)
}

/// Downloads the current backup.
///
/// # Errors
/// Returns `AppError::NotFound` if the backup does not exist.
/// Returns `AppError::Internal` if there is an error during download.
pub async fn download_backup(
    auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    // 1. Check If-None-Match for caching optimization
    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
        let if_none_match_version = if_none_match.trim_matches('"');
        if let Ok(version) = if_none_match_version.parse::<i32>() {
            // Fast-path: Check DB version before touching S3
            if let Some(current_version) = state.backup_service.get_current_version(auth_user.user_id).await?
                && current_version == version
            {
                return Ok(StatusCode::NOT_MODIFIED.into_response());
            }
        }
    }

    let (version, len, stream) = state.backup_service.download(auth_user.user_id).await?;

    // Bridge StorageStream -> Axum Body
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_str(&len.to_string()).map_err(|_| AppError::Internal)?);
    // Return ETag quoted
    response
        .headers_mut()
        .insert(header::ETAG, HeaderValue::from_str(&format!("\"{version}\"")).map_err(|_| AppError::Internal)?);

    Ok(response)
}

/// Checks for backup existence and returns metadata.
///
/// # Errors
/// Returns `AppError::NotFound` if the backup does not exist.
/// Returns `AppError::Internal` if there is an error.
pub async fn head_backup(auth_user: AuthUser, State(state): State<AppState>) -> Result<impl IntoResponse> {
    let (version, len) = state.backup_service.head(auth_user.user_id).await?;

    let mut response = Response::new(Body::empty());
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_str(&len.to_string()).map_err(|_| AppError::Internal)?);
    response
        .headers_mut()
        .insert(header::ETAG, HeaderValue::from_str(&format!("\"{version}\"")).map_err(|_| AppError::Internal)?);

    Ok(response)
}
