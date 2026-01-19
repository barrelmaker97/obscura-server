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

    let resp = app.client
        .post(format!("{}/v1/users", app.server_url))
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    
    let json: serde_json::Value = resp.json().await.unwrap();
    let access_token_1 = json["token"].as_str().unwrap().to_string();
    let refresh_token_1 = json["refreshToken"].as_str().expect("Missing refreshToken").to_string();

    // 2. Use Refresh Token to get new pair
    let refresh_payload = json!({
        "refreshToken": refresh_token_1
    });

    let resp_refresh = app.client
        .post(format!("{}/v1/sessions/refresh", app.server_url))
        .json(&refresh_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp_refresh.status(), StatusCode::OK);

    let json_refresh: serde_json::Value = resp_refresh.json().await.unwrap();
    let access_token_2 = json_refresh["token"].as_str().unwrap().to_string();
    let refresh_token_2 = json_refresh["refreshToken"].as_str().unwrap().to_string();

    // Assert rotation occurred
    // Access tokens might be identical if generated in the same second (same exp claim), so we don't strictly assert inequality.
    assert!(!access_token_1.is_empty());
    assert!(!access_token_2.is_empty());
    assert_ne!(refresh_token_1, refresh_token_2, "Refresh token should rotate");

    // 3. Verify Old Refresh Token is Invalid (Rotation Check)
    let resp_old_refresh = app.client
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

    let resp = app.client
        .post(format!("{}/v1/users", app.server_url))
        .json(&payload)
        .send()
        .await
        .unwrap();
    
    let json: serde_json::Value = resp.json().await.unwrap();
    let token = json["token"].as_str().unwrap();
    let refresh_token = json["refreshToken"].as_str().unwrap();

    // 2. Logout
    let resp_logout = app.client
        .delete(format!("{}/v1/sessions", app.server_url))
        .header("Authorization", format!("Bearer {}", token)) // Pass access token to identify session
        .json(&json!({ "refreshToken": refresh_token })) // Ideally logout invalids specific refresh token or all?
                                                         // Design decision: simple logout usually just revokes the RT. 
                                                         // If we require the RT in the body, it's explicit.
                                                         // Let's assume for now we might need to send it, or just use auth header.
                                                         // Following standard practice: DELETE /sessions usually invalidates the session associated with the caller.
                                                         // But wait, JWTs are stateless. So we MUST provide the refresh token to delete it from DB.
        .send()
        .await
        .unwrap();

    // We will implement Logout to require the Refresh Token in the body to know WHICH one to delete, 
    // OR we can delete ALL for the user if we just use the Bearer token.
    // Use Case: "Log out of this device" vs "Log out of all devices".
    // For "Log out of this device", we need the refresh token handle.
    // Let's implement: DELETE /sessions with body { refreshToken: "..." }
    
    assert_eq!(resp_logout.status(), StatusCode::OK);

    // 3. Try to Refresh after Logout
    let resp_fail = app.client
        .post(format!("{}/v1/sessions/refresh", app.server_url))
        .json(&json!({ "refreshToken": refresh_token }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_fail.status(), StatusCode::UNAUTHORIZED, "Refresh token should be revoked after logout");
}
