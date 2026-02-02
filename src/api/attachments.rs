use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::Result;
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

pub async fn upload_attachment(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse> {
    let content_len = headers.get(header::CONTENT_LENGTH).and_then(|v| match v.to_str() {
        Ok(s) => match s.parse::<usize>() {
            Ok(len) => Some(len),
            Err(e) => {
                tracing::debug!(error = %e, "Invalid Content-Length value");
                None
            }
        },
        Err(e) => {
            tracing::debug!(error = %e, "Invalid Content-Length encoding");
            None
        }
    });

    let (id, expires_at) = state.attachment_service.upload(content_len, body).await?;

    Ok((StatusCode::CREATED, Json(json!({ "id": id, "expiresAt": expires_at }))))
}

pub async fn download_attachment(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let (content_length, body_stream) = state.attachment_service.download(id).await?;

    let reader = body_stream.into_async_read();
    let stream = ReaderStream::new(reader);
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);

    if let Ok(val) = "application/octet-stream".parse() {
        response.headers_mut().insert(header::CONTENT_TYPE, val);
    } else {
        tracing::debug!("Failed to parse default Content-Type header value");
    }

    if content_length > 0 {
        if let Ok(val) = content_length.to_string().parse() {
            response.headers_mut().insert(header::CONTENT_LENGTH, val);
        } else {
            tracing::debug!(content_length = %content_length, "Failed to parse Content-Length header value");
        }
    }

    Ok(response)
}
