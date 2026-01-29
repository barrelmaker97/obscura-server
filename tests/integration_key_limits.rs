use base64::Engine;
use serde_json::json;
use uuid::Uuid;
use xeddsa::CalculateKeyPair;
use xeddsa::xed25519::PrivateKey;

mod common;

#[tokio::test]
async fn test_key_limit_enforced() {
    let mut config = common::get_test_config();
    config.messaging.max_pre_keys = 50; // Set low limit for testing
    let app = common::TestApp::spawn_with_config(config).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("limit_user_{}", run_id);

    let identity_key = common::generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = ik_pub_mont.to_vec();
    ik_pub_wire.insert(0, 0x05);

    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // 1. Register with 40 keys (Under limit)
    let mut keys = Vec::new();
    for i in 0..40 {
        keys.push(json!({
            "keyId": i,
            "publicKey": "BQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB" // 0x05 + 32x 0x01
        }));
    }

    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
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

    // 2. Refill with 20 keys (Total 60 > 50) -> Should succeed and PRUNE oldest
    let mut refill_keys = Vec::new();
    for i in 40..60 {
        refill_keys.push(json!({
            "keyId": i,
            "publicKey": "BQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB"
        }));
    }

    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&identity_key);

    let refill_payload = json!({
        // Same Identity Key = Refill
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
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

    assert_eq!(resp.status(), 200, "Refill should succeed with pruning");

    // 3. Verify total is capped at 50
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM one_time_pre_keys WHERE user_id IN (SELECT id FROM users WHERE username = $1)",
    )
    .bind(&username)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(count, 50, "Total keys should be capped at max_pre_keys (50)");
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
    let new_priv = PrivateKey(new_identity_key);
    let (_, new_ik_pub_ed) = new_priv.calculate_key_pair(0);
    let new_ik_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(new_ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut new_ik_pub_wire = new_ik_pub_mont.to_vec();
    new_ik_pub_wire.insert(0, 0x05);

    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&new_identity_key);

    let mut keys = Vec::new();
    for i in 0..20 {
        keys.push(json!({
            "keyId": i,
            "publicKey": "BQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB"
        }));
    }

    let takeover_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&new_ik_pub_wire),
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
