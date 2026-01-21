use base64::Engine;
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_prekey_status_low_keys() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("status_user_low_{}", run_id);

    // 1. Register with 0 one-time keys (below threshold of 20)
    // The default register_user_full sends empty oneTimePreKeys list
    let (token, _, _) = app.register_user_full(&username, 123).await;

    // 2. Connect WebSocket
    let mut ws = app.connect_ws(&token).await;

    // 3. Expect PreKeyStatus message immediately
    let status = ws.receive_prekey_status().await.expect("Did not receive PreKeyStatus");
    assert_eq!(status.one_time_pre_key_count, 0);
    assert_eq!(status.min_threshold, 20);
}

#[tokio::test]
async fn test_prekey_status_sufficient_keys() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("status_user_ok_{}", run_id);

    // 1. Register with 25 one-time keys (above threshold of 20)
    let mut rng = rand::rngs::OsRng;
    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes();
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    let mut keys = Vec::new();
    for i in 0..25 {
        keys.push(json!({
            "keyId": i,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=" // Dummy key
        }));
    }

    let payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": keys
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Connect WebSocket
    let mut ws = app.connect_ws(&token).await;

    // 3. Expect NO PreKeyStatus message
    let status = ws.receive_prekey_status_timeout(std::time::Duration::from_millis(500)).await;
    assert!(status.is_none(), "Received PreKeyStatus unexpectedly!");
}
