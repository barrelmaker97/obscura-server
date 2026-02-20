use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::api::schemas::attachments::AttachmentResponse;
use crate::error::{AppError, Result};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use uuid::Uuid;

/// Uploads an attachment to storage.
///
/// # Errors
/// Returns `AppError::Internal` if there is an error during upload.
pub async fn upload_attachment(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse> {
    let content_len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<usize>().ok()))
        .ok_or(AppError::LengthRequired)?;

    // Bridge Axum Body -> StorageStream (using neutral std::io::Error)
    let stream = body.into_data_stream().map(|res| res.map_err(|e| std::io::Error::other(e.to_string()))).boxed();

    let (id, expires_at) = state.attachment_service.upload(Some(content_len), stream).await?;

    Ok((StatusCode::CREATED, Json(AttachmentResponse { id, expires_at })))
}

/// Downloads an attachment from storage.
///
/// # Errors
/// Returns `AppError::NotFound` if the attachment is not found.
///
/// # Panics
/// Panics if the default Content-Type cannot be parsed.
pub async fn download_attachment(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let (content_length, stream) = state.attachment_service.download(id).await?;

    // Bridge StorageStream -> Axum Body
    let body = Body::from_stream(stream);
    let mut response = Response::new(body);

    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string()).map_err(|_| AppError::Internal)?,
    );

    Ok(response)
}
