use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use obscura_server::api::app_router;
use obscura_server::config::Config;
use obscura_server::storage;
use obscura_server::core::notification::InMemoryNotifier;
use serde_json::json;
use base64::Engine;
use uuid::Uuid;
use std::sync::Arc;

#[tokio::test]
async fn test_register_flow() {
    let config = Config {
        database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://user:pass@localhost/db".to_string()),
        jwt_secret: "secret".to_string(),
        rate_limit_per_second: 5,
        rate_limit_burst: 10,
    };

    // Mock or check connection. For now we assume failure if no DB.
    let pool_res = storage::init_pool(&config.database_url).await;
    if pool_res.is_err() {
        eprintln!("Skipping test: No DB connection");
        return;
    }
    let pool = pool_res.unwrap();

    let notifier = Arc::new(InMemoryNotifier::new());
    let app = app_router(pool, config, notifier);
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("testuser_reg_{}", run_id);

    // 1. Register
    let payload = json!({
        "username": username,
        "password": "password123",
        "registrationId": 123,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=", 
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/accounts")
        .header("content-type", "application/json")
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // 2. Login
    let login_payload = json!({
        "username": username,
        "password": "password123"
    });

    let req_login = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("content-type", "application/json")
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::from(serde_json::to_string(&login_payload).unwrap()))
        .unwrap();

    let response_login = app.clone().oneshot(req_login).await.unwrap();
    assert_eq!(response_login.status(), StatusCode::OK);
    
    // Verify token exists
    let body_bytes = axum::body::to_bytes(response_login.into_body(), usize::MAX).await.unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let token = body_json["token"].as_str().unwrap();
    
    // 3. Fetch Keys (Should fail due to empty one-time keys)
    let claims = decode_jwt_claims(token);
    let user_id = claims["sub"].as_str().unwrap();

    let req_keys = Request::builder()
        .method("GET")
        .uri(format!("/v1/keys/{}", user_id))
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::empty())
        .unwrap();

    let response_keys = app.clone().oneshot(req_keys).await.unwrap();
    assert_eq!(response_keys.status(), StatusCode::BAD_REQUEST);
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
