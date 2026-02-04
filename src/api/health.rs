use crate::api::MgmtState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

/// Liveness probe: returns 200 OK as long as the server is running.
pub async fn livez() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness probe: checks connectivity to the database and S3.
pub async fn readyz(State(state): State<MgmtState>) -> impl IntoResponse {
    let (db_res, s3_res) = tokio::join!(
        state.health_service.check_db(),
        state.health_service.check_s3()
    );

    let mut status_code = StatusCode::OK;
    let db_status = if let Err(e) = db_res {
        tracing::warn!(error = %e, component = "database", "Readiness probe failed");
        status_code = StatusCode::SERVICE_UNAVAILABLE;
        "error"
    } else {
        "ok"
    };

    let s3_status = if let Err(e) = s3_res {
        tracing::warn!(error = %e, component = "s3", "Readiness probe failed");
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