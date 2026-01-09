use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use obscura_server::api::app_router;
use obscura_server::config::Config;
use obscura_server::storage;
use serde_json::json;

#[tokio::test]
async fn test_register_flow() {
    let config = Config {
        database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://user:pass@localhost/db".to_string()),
        jwt_secret: "secret".to_string(),
    };

    // Mock or check connection. For now we assume failure if no DB.
    let pool_res = storage::init_pool(&config.database_url).await;
    if pool_res.is_err() {
        eprintln!("Skipping test: No DB connection");
        return;
    }
    let pool = pool_res.unwrap();

    let app = app_router(pool, config);

    // 1. Register
    let payload = json!({
        "username": "testuser_reg", // Unique name
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
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}
