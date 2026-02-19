use reqwest::StatusCode;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_attachment_lifecycle() {
    let mut config = common::get_test_config();
    config.storage.bucket = format!("test-bucket-{}", &Uuid::new_v4().to_string()[..8]);
    config.attachment.max_size_bytes = 100; // Small but enough for "hello"

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("att_life_{}", run_id)).await;

    // 1. Upload Success
    let content = b"Hello Obscura!";
    let resp_up = app
        .client
        .post(format!("{}/v1/attachments", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Length", content.len().to_string())
        .body(content.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_up.status(), StatusCode::CREATED);
    let up_json: serde_json::Value = resp_up.json().await.unwrap();
    let attachment_id = up_json["id"].as_str().unwrap();

    // 2. Download Success
    let resp_down = app
        .client
        .get(format!("{}/v1/attachments/{}", app.server_url, attachment_id))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_down.status(), StatusCode::OK);
    assert_eq!(resp_down.bytes().await.unwrap(), content.to_vec());

    // 3. Upload Failure (Size Limit - Header check)
    let resp_big_header = app
        .client
        .post(format!("{}/v1/attachments", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Length", "101") // Over 100 limit
        .body(vec![0u8; 101])
        .send()
        .await
        .unwrap();
    assert_eq!(resp_big_header.status(), StatusCode::BAD_REQUEST);

    // 4. Upload Failure (Size Limit - Stream check)
    let stream_data = vec![0u8; 150];
    let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(axum::body::Bytes::from(stream_data))]);
    let resp_big_stream = app
        .client
        .post(format!("{}/v1/attachments", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .unwrap();

    // Server should hit the Limited body wrapper and return 413 or 500 depending on exactly where it fails
    // In current impl it returns 500 because the S3 upload task fails due to early stream termination
    assert_eq!(resp_big_stream.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // 5. Download Not Found
    let resp_404 = app
        .client
        .get(format!("{}/v1/attachments/{}", app.server_url, Uuid::new_v4()))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);
}
