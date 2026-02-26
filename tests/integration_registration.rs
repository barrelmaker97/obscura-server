#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::todo,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    missing_debug_implementations,
    clippy::cast_precision_loss,
    clippy::clone_on_ref_ptr,
    clippy::match_same_arms,
    clippy::items_after_statements,
    unreachable_pub,
    clippy::print_stdout,
    clippy::similar_names
)]
use base64::Engine;
use reqwest::StatusCode;
use serde_json::json;
use xeddsa::CalculateKeyPair;
use xeddsa::xed25519::PrivateKey;

mod common;

#[tokio::test]
async fn test_register_flow() {
    let app = common::TestApp::spawn().await;
    let username = common::generate_username("user");

    let identity_key = common::generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = ik_pub_mont.to_vec();
    ik_pub_wire.insert(0, 0x05);

    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    let payload = json!({
        "username": username,
        "password": "password12345",
        "registrationId": 123,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(ik_pub_wire),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": [
            {
                "keyId": 1,
                "publicKey": "BQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB"
            },
            {
                "keyId": 2,
                "publicKey": "BQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB"
            }
        ]
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();

    assert_eq!(resp.status(), 201);

    // Verify response structure
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json.get("token").is_some());
    assert!(json.get("refreshToken").is_some(), "Registration response must include refreshToken");
    assert!(json.get("expiresAt").is_some(), "Registration response must include expiresAt");

    // 2. Login
    let login_payload = json!({
        "username": username,
        "password": "password12345",
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
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_keys.status(), StatusCode::OK);
}
