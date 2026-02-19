use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::{AppError, Result};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio_util::io::ReaderStream;

/// Uploads a new backup version.
///
/// # Errors
/// Returns `AppError::BadRequest` if `If-Match` header is missing or invalid.
/// Returns `AppError::PreconditionFailed` if `If-Match` version does not match current version.
/// Returns `AppError::Conflict` if another upload is in progress.
pub async fn upload_backup(
    auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse> {
    let if_match_header = headers
        .get("If-Match")
        .ok_or(AppError::BadRequest("Missing If-Match header".into()))?
        .to_str()
        .map_err(|_| AppError::BadRequest("Invalid If-Match header".into()))?;
        
    let if_match_str = if_match_header.trim_matches('"');
    let if_match_version = if_match_str.parse::<i32>()
        .map_err(|_| AppError::BadRequest("Invalid version in If-Match header".into()))?;

    let content_len = headers.get(header::CONTENT_LENGTH).and_then(|v| {
        v.to_str().ok().and_then(|s| s.parse::<usize>().ok())
    });

    state
        .backup_service
        .handle_upload(auth_user.user_id, if_match_version, content_len, body)
        .await?;

    Ok(StatusCode::OK)
}

/// Downloads the current backup.
///
/// # Errors
/// Returns `AppError::NotFound` if no backup exists.
/// Returns `AppError::Internal` if response headers cannot be constructed.
pub async fn download_backup(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<impl IntoResponse> {
    let (version, len, stream) = state.backup_service.download(auth_user.user_id).await?;
    
    let reader = stream.into_async_read();
    let stream = ReaderStream::new(reader);
    let body = Body::from_stream(stream);
    
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE, 
        HeaderValue::from_static("application/octet-stream")
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH, 
        HeaderValue::from_str(&len.to_string()).map_err(|_| AppError::Internal)?
    );
    // Return ETag quoted
    response.headers_mut().insert(
        header::ETAG, 
        HeaderValue::from_str(&format!("\"{version}\"")).map_err(|_| AppError::Internal)?
    );
    
    Ok(response)
}

/// Checks for backup existence and returns metadata.
///
/// # Errors
/// Returns `AppError::NotFound` if no backup exists.
/// Returns `AppError::Internal` if response headers cannot be constructed.
pub async fn head_backup(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<impl IntoResponse> {
    let (version, len) = state.backup_service.head(auth_user.user_id).await?;
    
    let mut response = Response::new(Body::empty());
    response.headers_mut().insert(
        header::CONTENT_TYPE, 
        HeaderValue::from_static("application/octet-stream")
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH, 
        HeaderValue::from_str(&len.to_string()).map_err(|_| AppError::Internal)?
    );
    response.headers_mut().insert(
        header::ETAG, 
        HeaderValue::from_str(&format!("\"{version}\"")).map_err(|_| AppError::Internal)?
    );
    
    Ok(response)
}
