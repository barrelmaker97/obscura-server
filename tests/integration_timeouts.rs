use futures::stream;
use reqwest::StatusCode;
use std::time::Duration;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_standard_request_timeout() {
    let mut config = common::get_test_config();
    // Set standard request timeout to 2s for better CI stability
    config.server.request_timeout_secs = 2;

    let app = common::TestApp::spawn_with_config(config).await;
    let username = format!("timeout_std_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // Create a stream that waits 3 seconds (exceeding 2s timeout)
    let delayed_stream = stream::unfold(0, |state| async move {
        if state == 0 {
            tokio::time::sleep(Duration::from_millis(3000)).await;
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from("slow data")), 1))
        } else {
            None
        }
    });

    let resp = app
        .client
        .post(format!("{}/v1/messages", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Idempotency-Key", Uuid::new_v4().to_string())
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", "9")
        .body(reqwest::Body::wrap_stream(delayed_stream))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_backup_upload_timeout() {
    let mut config = common::get_test_config();
    config.backup.request_timeout_secs = 2; // 2 second limit

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let username = format!("timeout_up_backup_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // Create a stream that waits 3 seconds before sending data
    let delayed_stream = stream::unfold(0, |state| async move {
        if state == 0 {
            tokio::time::sleep(Duration::from_millis(3000)).await;
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from(vec![0u8; 40])), 1))
        } else {
            None
        }
    });

    let resp = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-None-Match", "*")
        .header("Content-Length", "40")
        .body(reqwest::Body::wrap_stream(delayed_stream))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_attachment_upload_timeout() {
    let mut config = common::get_test_config();
    config.attachment.request_timeout_secs = 2; // 2 second limit

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let username = format!("timeout_up_att_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // Create a stream that waits 3 seconds
    let delayed_stream = stream::unfold(0, |state| async move {
        if state == 0 {
            tokio::time::sleep(Duration::from_millis(3000)).await;
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from("att data")), 1))
        } else {
            None
        }
    });

    let resp = app
        .client
        .post(format!("{}/v1/attachments", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Length", "8")
        .body(reqwest::Body::wrap_stream(delayed_stream))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_global_safety_timeout() {
    let mut config = common::get_test_config();
    config.server.global_timeout_secs = 3; // Hard cap at 3s to allow registration
    // Make standard timeout longer so only global trips
    config.server.request_timeout_secs = 30;

    let app = common::TestApp::spawn_with_config(config).await;
    let username = format!("timeout_global_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // Create a stream that takes 4 seconds (longer than global 3s, but shorter than request 30s)
    let delayed_stream = stream::unfold(0, |state| async move {
        if state == 0 {
            tokio::time::sleep(Duration::from_millis(4000)).await;
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from("slow data")), 1))
        } else {
            None
        }
    });

    let resp = app
        .client
        .post(format!("{}/v1/attachments", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Length", "9")
        .body(reqwest::Body::wrap_stream(delayed_stream))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_attachment_timeout_independence() {
    let mut config = common::get_test_config();

    // CRITICAL: Set Attachment Timeout LONGER than Backup Timeout
    // If the bug exists, the shorter Backup timeout (1s) will kill the Attachment request
    config.attachment.request_timeout_secs = 5;
    config.backup.request_timeout_secs = 1;

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let username = format!("timeout_indep_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // Create a stream that takes 2 seconds (Longer than Backup's 1s, shorter than Attachment's 5s)
    let delayed_stream = stream::unfold(0, |state| async move {
        if state == 0 {
            tokio::time::sleep(Duration::from_millis(2000)).await;
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from("slow data")), 1))
        } else {
            None
        }
    });

    let resp = app
        .client
        .post(format!("{}/v1/attachments", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Length", "9")
        .body(reqwest::Body::wrap_stream(delayed_stream))
        .send()
        .await
        .unwrap();

    // If bug exists: 408 Request Timeout (killed by 1s backup layer)
    // If fixed: 201 Created (allowed by 5s attachment layer)
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Attachment upload should have succeeded with 5s timeout, but likely failed due to 1s backup timeout wrapping it."
    );
}
