#![allow(clippy::unwrap_used, clippy::panic, clippy::todo, clippy::missing_panics_doc, clippy::must_use_candidate, missing_debug_implementations, clippy::cast_precision_loss, clippy::clone_on_ref_ptr, clippy::match_same_arms, clippy::items_after_statements, unreachable_pub, clippy::print_stdout, clippy::similar_names)]
use axum::http::StatusCode;
mod common;

#[tokio::test]
async fn test_livez() {
    let app = common::TestApp::spawn().await;

    let resp = app.client.get(format!("{}/livez", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_readyz_happy_path() {
    let app = common::TestApp::spawn().await;

    // Ensure the bucket exists so storage check passes
    common::ensure_storage_bucket(&app.s3_client, &app.config.storage.bucket).await;

    let resp = app.client.get(format!("{}/readyz", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["storage"], "ok");
}

#[tokio::test]
async fn test_readyz_storage_error() {
    let mut config = common::get_test_config();
    // Use a bucket name that definitely won't exist and we won't create
    config.storage.bucket = format!("non-existent-bucket-{}", uuid::Uuid::new_v4());

    let app = common::TestApp::spawn_with_config(config).await;

    let resp = app.client.get(format!("{}/readyz", app.mgmt_url)).send().await.unwrap();

    // Storage check should fail because the bucket doesn't exist
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["storage"], "error");
}

#[tokio::test]
async fn test_readyz_database_error() {
    let app = common::TestApp::spawn().await;
    common::ensure_storage_bucket(&app.s3_client, &app.config.storage.bucket).await;

    // Close the pool to simulate a database error
    app.pool.close().await;

    let resp = app.client.get(format!("{}/readyz", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert_eq!(body["database"], "error");
    assert_eq!(body["storage"], "ok");
}
