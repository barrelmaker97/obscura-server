use crate::api::MgmtState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use opentelemetry::{global, KeyValue};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Liveness probe: returns 200 OK as long as the server is running.
pub async fn livez() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness probe: checks connectivity to the database and S3.
pub async fn readyz(State(state): State<MgmtState>) -> impl IntoResponse {
    let db_timeout = Duration::from_millis(state.health_config.db_timeout_ms);
    let s3_timeout = Duration::from_millis(state.health_config.s3_timeout_ms);
    let meter = global::meter("obscura-server");
    let histogram = meter.f64_histogram("health_check_duration_seconds").with_description("Duration of health checks").build();

    let db_check = async {
        let start = Instant::now();
        let res = match timeout(db_timeout, sqlx::query("SELECT 1").execute(&state.pool)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("Database connection failed: {:?}", e)),
            Err(_) => Err("Database connection timed out".to_string()),
        };
        histogram.record(start.elapsed().as_secs_f64(), &[KeyValue::new("component", "database")]);
        res
    };

    let s3_check = async {
        let start = Instant::now();
        let res = match timeout(s3_timeout, state.s3_client.head_bucket().bucket(&state.s3_bucket).send()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("S3 connection failed for bucket {}: {:?}", state.s3_bucket, e)),
            Err(_) => Err("S3 connection timed out".to_string()),
        };
        histogram.record(start.elapsed().as_secs_f64(), &[KeyValue::new("component", "s3")]);
        res
    };

    let (db_res, s3_res) = tokio::join!(db_check, s3_check);

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
