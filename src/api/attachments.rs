use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::{AppError, Result};
use aws_sdk_s3::primitives::ByteStream;
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use sqlx::Row;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

// Wrapper to satisfy S3 SDK's Sync requirement for Body
struct SyncBody {
    rx: Arc<Mutex<mpsc::Receiver<std::result::Result<Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>>>>,
}

impl http_body::Body for SyncBody {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let mut rx = match self.rx.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };

        match rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(http_body::Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub async fn upload_attachment(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse> {
    // 1. Check Content-Length
    let content_len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    if content_len > state.config.attachment_max_size_bytes {
        return Err(AppError::BadRequest("Attachment too large".into()));
    }

    let id = Uuid::new_v4();
    let key = id.to_string();

    // 2. Bridge Axum Body -> SyncBody
    let (tx, rx) = mpsc::channel(2); // Small buffer
    let mut data_stream = body.into_data_stream();

    tokio::spawn(async move {
        use futures::StreamExt;
        while let Some(item) = data_stream.next().await {
            // Convert axum::Error to Box<dyn Error...>
            let item = item.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>);
            if tx.send(item).await.is_err() {
                break;
            }
        }
    });

    let sync_body = SyncBody { rx: Arc::new(Mutex::new(rx)) };
    let byte_stream = ByteStream::from_body_1_x(sync_body);

    state.s3_client.put_object().bucket(&state.config.s3_bucket).key(&key).body(byte_stream).send().await.map_err(
        |e| {
            tracing::error!("S3 Upload failed: {:?}", e);
            AppError::Internal
        },
    )?;

    // 3. Record Metadata
    let expires_at = OffsetDateTime::now_utc() + Duration::days(state.config.attachment_ttl_days);

    sqlx::query("INSERT INTO attachments (id, expires_at) VALUES ($1, $2)")
        .bind(id)
        .bind(expires_at)
        .execute(&state.pool)
        .await?;

    Ok((StatusCode::CREATED, Json(json!({ "id": id, "expiresAt": expires_at.unix_timestamp() }))))
}

pub async fn download_attachment(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    // 1. Check Existence & Expiry
    let row =
        sqlx::query("SELECT expires_at FROM attachments WHERE id = $1").bind(id).fetch_optional(&state.pool).await?;

    match row {
        Some(r) => {
            let expires_at: OffsetDateTime = r.get("expires_at");
            if expires_at < OffsetDateTime::now_utc() {
                return Err(AppError::NotFound);
            }
        }
        None => return Err(AppError::NotFound),
    }

    // 2. Stream from S3
    let key = id.to_string();
    let output = state.s3_client.get_object().bucket(&state.config.s3_bucket).key(&key).send().await.map_err(|e| {
        tracing::error!("S3 Download failed for {}: {:?}", key, e);
        AppError::NotFound
    })?;

    // 3. Construct Response
    let content_length = output.content_length.unwrap_or(0);

    // Convert ByteStream (AsyncRead) -> Stream -> Body
    let reader = output.body.into_async_read();
    let stream = ReaderStream::new(reader);
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    response.headers_mut().insert(header::CONTENT_TYPE, "application/octet-stream".parse().unwrap());
    if content_length > 0 {
        response.headers_mut().insert(header::CONTENT_LENGTH, content_length.to_string().parse().unwrap());
    }

    Ok(response)
}
