use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_refresh_token_flow() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("refresh_user_{}", run_id);

    // 1. Register and get initial tokens
    let user = app.register_user(&username).await;

    // 2. Use Refresh Token to get new pair
    let refresh_payload = json!({
        "refreshToken": user.refresh_token
    });

    let resp_refresh =
        app.client.post(format!("{}/v1/sessions/refresh", app.server_url)).json(&refresh_payload).send().await.unwrap();

    assert_eq!(resp_refresh.status(), StatusCode::OK);

    let json_refresh: serde_json::Value = resp_refresh.json().await.unwrap();
    let access_token_2 = json_refresh["token"].as_str().unwrap().to_string();
    let refresh_token_2 = json_refresh["refreshToken"].as_str().unwrap().to_string();

    // Assert rotation occurred
    assert!(!user.token.is_empty());
    assert!(!access_token_2.is_empty());
    assert_ne!(user.refresh_token, refresh_token_2, "Refresh token should rotate");

    // 3. Verify Old Refresh Token is Invalid (Rotation Check)
    let resp_old_refresh = app
        .client
        .post(format!("{}/v1/sessions/refresh", app.server_url))
        .json(&refresh_payload) // sending refresh_token_1 again
        .send()
        .await
        .unwrap();

    assert_eq!(resp_old_refresh.status(), StatusCode::UNAUTHORIZED, "Old refresh token should be invalidated");
}

#[tokio::test]
async fn test_logout_revokes_refresh_token() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("logout_user_{}", run_id);

    // 1. Register
    let user = app.register_user(&username).await;

    // 2. Logout
    let resp_logout = app
        .client
        .delete(format!("{}/v1/sessions", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&json!({ "refreshToken": user.refresh_token }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_logout.status(), StatusCode::OK);

    // 3. Try to Refresh after Logout
    let resp_fail = app
        .client
        .post(format!("{}/v1/sessions/refresh", app.server_url))
        .json(&json!({ "refreshToken": user.refresh_token }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_fail.status(), StatusCode::UNAUTHORIZED, "Refresh token should be revoked after logout");
}

#[tokio::test]
async fn test_refresh_token_expiration() {
    // 1. Setup app with 0-day TTL (immediate expiration)
    let mut config = common::get_test_config();
    config.auth.refresh_token_ttl_days = 0;

    let app = common::TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("expire_user_{}", run_id);

    // 2. Register
    let user = app.register_user(&username).await;

    // 3. Wait a moment to ensure clock ticks (1s)
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // 4. Try to Refresh (Should fail)
    let refresh_payload = json!({
        "refreshToken": user.refresh_token
    });

    let resp_refresh =
        app.client.post(format!("{}/v1/sessions/refresh", app.server_url)).json(&refresh_payload).send().await.unwrap();

    assert_eq!(resp_refresh.status(), StatusCode::UNAUTHORIZED, "Expired refresh token should be rejected");
}