use obscura_server::api::app_router;
use obscura_server::core::notification::InMemoryNotifier;
use reqwest::StatusCode;
use std::sync::Arc;
use tokio::net::TcpListener;
use uuid::Uuid;

mod common;

async fn ensure_bucket(s3_client: &aws_sdk_s3::Client, bucket: &str) {
    let _ = s3_client.create_bucket().bucket(bucket).send().await;
}

#[tokio::test]
async fn test_attachment_lifecycle() {
    let mut config = common::get_test_config();
    config.s3.endpoint = Some("http://localhost:9000".to_string());
    config.s3.bucket = format!("test-bucket-{}", &Uuid::new_v4().to_string()[..8]);
    config.s3.force_path_style = true;
    config.s3.attachment_max_size_bytes = 100; // Small but enough for "hello"

    // Init S3 Client
    let region_provider = aws_config::Region::new(config.s3.region.clone());
    let config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(region_provider)
        .endpoint_url(config.s3.endpoint.as_ref().unwrap())
        .credentials_provider(aws_credential_types::Credentials::new("minioadmin", "minioadmin", None, None, "static"));

    let sdk_config = config_loader.load().await;
    let s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(true);
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config_builder.build());

    ensure_bucket(&s3_client, &config.s3.bucket).await;

    // Init App
    let pool = common::get_test_pool().await;
    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool.clone(), config.clone(), notifier, s3_client.clone());

    // Spawn Server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let client = reqwest::Client::new();
    let app_helper = common::TestApp {
        pool,
        config: config.clone(),
        server_url: server_url.clone(),
        ws_url: "".to_string(),
        client: client.clone(),
    };
    let username = format!("att_life_{}", &Uuid::new_v4().to_string()[..8]);
    let (token, _) = app_helper.register_user(&username).await;

    // 1. Upload Success
    let content = b"Hello Obscura!";
    let resp_up = client
        .post(format!("{}/v1/attachments", server_url))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Length", content.len().to_string())
        .body(content.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_up.status(), StatusCode::CREATED);
    let up_json: serde_json::Value = resp_up.json().await.unwrap();
    let attachment_id = up_json["id"].as_str().unwrap();

    // 2. Download Success
    let resp_down = client
        .get(format!("{}/v1/attachments/{}", server_url, attachment_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_down.status(), StatusCode::OK);
    assert_eq!(resp_down.bytes().await.unwrap(), content.to_vec());

    // 3. Upload Failure (Size Limit - Header check)
    let resp_big_header = client
        .post(format!("{}/v1/attachments", server_url))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Length", "101") // Over 100 limit
        .body(vec![0u8; 101])
        .send()
        .await
        .unwrap();
    assert_eq!(resp_big_header.status(), StatusCode::BAD_REQUEST);

    // 4. Upload Failure (Size Limit - Stream check)
    // We use a stream to bypass the Content-Length header check.
    let stream_data = vec![0u8; 150];
    let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(axum::body::Bytes::from(stream_data))]);
    let resp_big_stream = client
        .post(format!("{}/v1/attachments", server_url))
        .header("Authorization", format!("Bearer {}", token))
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .unwrap();

    // Server should hit the Limited body wrapper, fail the S3 upload, and return 500
    assert_eq!(resp_big_stream.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // 5. Download Not Found
    let resp_404 = client
        .get(format!("{}/v1/attachments/{}", server_url, Uuid::new_v4()))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);
}
