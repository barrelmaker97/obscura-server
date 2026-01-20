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
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
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

    let refill_payload = json!({
        // Same Identity Key = Refill
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=", 
        "registrationId": 123,
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": refill_keys
    });

    let resp = app.client.post(format!("{}/v1/keys", app.server_url))
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
    let (token, _) = app.register_user(&username).await;

    // 2. Takeover with 20 keys (More than limit of 10)
    // Even in takeover, the new set of keys should not exceed the limit.
    
    let mut keys = Vec::new();
    for i in 0..20 {
        keys.push(json!({
            "keyId": i,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        }));
    }

    let takeover_payload = json!({
        "identityKey": "bmV3X2lkZW50aXR5X2tleQ==", // Changed ID Key
        "registration_id": 456,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": keys
    });

    let resp = app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();
    
    // Should now be 400 because we enforced the limit in KeyService
    assert_eq!(resp.status(), 400);
}
