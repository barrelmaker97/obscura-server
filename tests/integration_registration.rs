use base64::Engine;
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_register_flow() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("testuser_reg_{}", run_id);

    // 1. Register (Manual to verify status code)
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

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify response structure
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json.get("token").is_some());
    assert!(json.get("refreshToken").is_some(), "Registration response must include refreshToken");
    assert!(json.get("expiresAt").is_some(), "Registration response must include expiresAt");

    // 2. Login
    let login_payload = json!({
        "username": username,
        "password": "password123"
    });

    let resp_login =
        app.client.post(format!("{}/v1/sessions", app.server_url)).json(&login_payload).send().await.unwrap();

    assert_eq!(resp_login.status(), StatusCode::OK);

    let body_json: serde_json::Value = resp_login.json().await.unwrap();
    let token = body_json["token"].as_str().unwrap();

    // 3. Fetch Keys (Should fail due to empty one-time keys)
    // Decode token to get user ID
    let parts: Vec<&str> = token.split('.').collect();
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    let user_id = claims["sub"].as_str().unwrap();

    let resp_keys = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, user_id))
        .send() // No auth needed for key fetch
        .await
        .unwrap();

    assert_eq!(resp_keys.status(), StatusCode::BAD_REQUEST);
}
