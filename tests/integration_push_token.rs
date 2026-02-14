mod common;

use common::TestApp;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn test_register_push_token() {
    let app = TestApp::spawn().await;
    let username = format!("token_user_{}", Uuid::new_v4());
    let user = app.register_user(&username).await;

    let token = "test_fcm_token_123";
    let payload = json!({
        "token": token
    });

    let resp = app
        .client
        .put(format!("{}/v1/push/token", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Verify in database
    let stored_token: String = sqlx::query_scalar("SELECT token FROM push_tokens WHERE user_id = $1")
        .bind(user.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    assert_eq!(stored_token, token);

    // Update token
    let new_token = "updated_fcm_token_456";
    let resp = app
        .client
        .put(format!("{}/v1/push/token", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&json!({ "token": new_token }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let stored_token: String = sqlx::query_scalar("SELECT token FROM push_tokens WHERE user_id = $1")
        .bind(user.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    assert_eq!(stored_token, new_token);
}

#[tokio::test]
async fn test_register_push_token_unauthorized() {
    let app = TestApp::spawn().await;

    let resp = app
        .client
        .put(format!("{}/v1/push/token", app.server_url))
        .json(&json!({ "token": "some_token" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
}
