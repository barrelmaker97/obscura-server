use crate::api::MgmtState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

/// Liveness probe: returns 200 OK as long as the server is running.
pub async fn livez() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness probe: checks connectivity to the database and S3.
pub async fn readyz(State(state): State<MgmtState>) -> impl IntoResponse {
    let db_timeout = Duration::from_millis(state.config.health.db_timeout_ms);
    let s3_timeout = Duration::from_millis(state.config.health.s3_timeout_ms);

    let db_check = async {
        match timeout(db_timeout, sqlx::query("SELECT 1").execute(&state.pool)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("Database connection failed: {:?}", e)),
            Err(_) => Err("Database connection timed out".to_string()),
        }
    };

    let s3_check = async {
        match timeout(s3_timeout, state.s3_client.head_bucket().bucket(&state.config.s3.bucket).send()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("S3 connection failed for bucket {}: {:?}", state.config.s3.bucket, e)),
            Err(_) => Err("S3 connection timed out".to_string()),
        }
    };

    let (db_res, s3_res) = tokio::join!(db_check, s3_check);

    let mut status_code = StatusCode::OK;
    let db_status = if let Err(e) = db_res {
        tracing::warn!("Health check failure (Readyz): {}", e);
        status_code = StatusCode::SERVICE_UNAVAILABLE;
        "error"
    } else {
        "ok"
    };

    let s3_status = if let Err(e) = s3_res {
        tracing::warn!("Health check failure (Readyz): {}", e);
        status_code = StatusCode::SERVICE_UNAVAILABLE;
        "error"
    } else {
        "ok"
    };

    (
        status_code,
        Json(json!({
            "status": if status_code == StatusCode::OK { "ok" } else { "error" },
            "database": db_status,
            "s3": s3_status,
        })),
    )
}

/// Placeholder for future metrics endpoint.
pub async fn metrics() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Metrics endpoint not implemented yet")
}
