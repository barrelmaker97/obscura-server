use crate::api::MgmtState;
use crate::api::dto::health::HealthResponse;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

/// Liveness probe: returns 200 OK as long as the server is running.
pub async fn livez() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness probe: checks connectivity to the database and S3.
pub async fn readyz(State(state): State<MgmtState>) -> impl IntoResponse {
    let (db_res, storage_res) = tokio::join!(
        state.health_service.check_db(),
        state.health_service.check_storage()
    );

    let mut status_code = StatusCode::OK;
    let db_status = if let Err(e) = db_res {
        tracing::warn!(error = %e, component = "database", "Readiness probe failed");
        status_code = StatusCode::SERVICE_UNAVAILABLE;
        "error"
    } else {
        "ok"
    };

    let storage_status = if let Err(e) = storage_res {
        tracing::warn!(error = %e, component = "storage", "Readiness probe failed");
        status_code = StatusCode::SERVICE_UNAVAILABLE;
        "error"
    } else {
        "ok"
    };

    let response = HealthResponse {
        status: if status_code == StatusCode::OK { "ok" } else { "error" }.to_string(),
        database: db_status.to_string(),
        storage: storage_status.to_string(),
    };

    (status_code, Json(response))
}
