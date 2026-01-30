use axum::http::StatusCode;
mod common;

async fn ensure_bucket(s3_client: &aws_sdk_s3::Client, bucket: &str) {
    let _ = s3_client.create_bucket().bucket(bucket).send().await;
}

#[tokio::test]
async fn test_livez() {
    let app = common::TestApp::spawn().await;

    let resp = app.client.get(format!("{}/livez", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_metrics_placeholder() {
    let app = common::TestApp::spawn().await;

    let resp = app.client.get(format!("{}/metrics", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn test_readyz_happy_path() {
    let app = common::TestApp::spawn().await;

    // Ensure the bucket exists so S3 check passes
    ensure_bucket(&app.s3_client, &app.config.s3.bucket).await;

    let resp = app.client.get(format!("{}/readyz", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["s3"], "ok");
}

#[tokio::test]
async fn test_readyz_s3_error() {
    let mut config = common::get_test_config();
    // Use a bucket name that definitely won't exist and we won't create
    config.s3.bucket = format!("non-existent-bucket-{}", uuid::Uuid::new_v4());

    let app = common::TestApp::spawn_with_config(config).await;

    let resp = app.client.get(format!("{}/readyz", app.mgmt_url)).send().await.unwrap();

    // S3 check should fail because the bucket doesn't exist
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["s3"], "error");
}

#[tokio::test]
async fn test_readyz_database_error() {
    let app = common::TestApp::spawn().await;
    ensure_bucket(&app.s3_client, &app.config.s3.bucket).await;

    // Close the pool to simulate a database error
    app.pool.close().await;

    let resp = app.client.get(format!("{}/readyz", app.mgmt_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert_eq!(body["database"], "error");
    assert_eq!(body["s3"], "ok");
}
