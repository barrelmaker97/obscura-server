use base64::Engine;
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_key_limit_enforced() {
    let mut config = common::get_test_config();
    config.messaging.max_pre_keys = 50; // Set low limit for testing
    let app = common::TestApp::spawn_with_config(config).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("limit_user_{}", run_id);

    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes();
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // 1. Register with 40 keys (Under limit)
    let mut keys = Vec::new();
    for i in 0..40 {
        keys.push(json!({
            "keyId": i,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        }));
    }

    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(ik_pub),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": keys
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Refill with 20 keys (Total 60 > 50) -> Should Fail
    let mut refill_keys = Vec::new();
    for i in 40..60 {
        refill_keys.push(json!({
            "keyId": i,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        }));
    }

    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&identity_key);

    let refill_payload = json!({
        // Same Identity Key = Refill
        "identityKey": base64::engine::general_purpose::STANDARD.encode(ik_pub),
        "registrationId": 123,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&new_spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": refill_keys
    });

    let resp = app
        .client
        .post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&refill_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_key_limit_enforced_on_takeover() {
    let mut config = common::get_test_config();
    config.messaging.max_pre_keys = 10; // Very low limit
    let app = common::TestApp::spawn_with_config(config).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("takeover_limit_user_{}", run_id);

    // 1. Register
    let user = app.register_user(&username).await;

    // 2. Takeover with 20 keys (More than limit of 10)
    let new_identity_key = common::generate_signing_key();
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&new_identity_key);

    let mut keys = Vec::new();
    for i in 0..20 {
        keys.push(json!({
            "keyId": i,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        }));
    }

    let takeover_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(new_identity_key.verifying_key().to_bytes()),
        "registrationId": 456,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": keys
    });

    let resp = app
        .client
        .post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();

    // Should now be 400 because we enforced the limit in KeyService
    assert_eq!(resp.status(), 400);
}
