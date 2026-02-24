#![allow(clippy::unwrap_used, clippy::panic, clippy::todo)]
mod common;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::SinkExt;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;
use xeddsa::CalculateKeyPair;
use xeddsa::xed25519::PrivateKey;

#[tokio::test]
async fn test_multi_node_notification() {
    let config = common::get_test_config();
    let app_a = common::TestApp::spawn_with_config(config.clone()).await;
    let app_b = common::TestApp::spawn_with_config(config).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let alice_name = format!("alice_{}", run_id);
    let bob_name = format!("bob_{}", run_id);

    let user_alice = app_a.register_user(&alice_name).await;
    let user_bob = app_b.register_user(&bob_name).await;

    let mut ws_bob = app_b.connect_ws(&user_bob.token).await;

    let content = b"Cross-node message".to_vec();
    app_a.send_message(&user_alice.token, user_bob.user_id, &content).await;

    let env = ws_bob.receive_envelope().await.expect("Bob did not receive message on Node B");
    assert_eq!(env.message, content);
}

#[tokio::test]
async fn test_multi_node_push_cancellation() {
    let config = common::get_test_config();
    let app_a = common::TestApp::spawn_with_config(config.clone()).await;
    let app_b = common::TestApp::spawn_with_config(config.clone()).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app_a.register_user(&format!("multi_cancel_{}", run_id)).await;

    // 1. Manually schedule a push on the shared Redis queue
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let pubsub = obscura_server::adapters::redis::RedisClient::new(&app_a.config.pubsub, 1024, shutdown_rx.clone())
        .await
        .unwrap();

    let mut conn = pubsub.publisher();
    let run_at = time::OffsetDateTime::now_utc().unix_timestamp() + 30;
    let queue_key = config.notifications.push_queue_key.clone();

    redis::cmd("ZADD")
        .arg(&queue_key)
        .arg("NX")
        .arg(run_at as f64)
        .arg(user.user_id.to_string())
        .query_async::<i64>(&mut conn)
        .await
        .unwrap();

    // 2. Connect user to Node B
    let _ws = app_b.connect_ws(&user.token).await;

    // 3. Verify Node B cancelled the push scheduled by Node A
    let success = app_b
        .wait_until(
            || {
                let pubsub = pubsub.clone();
                let user_id = user.user_id;
                let queue_key = queue_key.clone();
                async move {
                    let mut conn = pubsub.publisher();
                    let score: Option<f64> = redis::cmd("ZSCORE")
                        .arg(&queue_key)
                        .arg(user_id.to_string())
                        .query_async(&mut conn)
                        .await
                        .unwrap();
                    score.is_none()
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(success, "Node B failed to cancel push scheduled by Node A");

    let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn test_multi_node_disconnect_notification() {
    let config = common::get_test_config();
    let app_a = common::TestApp::spawn_with_config(config.clone()).await;
    let app_b = common::TestApp::spawn_with_config(config).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let alice_name = format!("alice_takeover_{}", run_id);

    let user_alice = app_a.register_user(&alice_name).await;

    let mut ws_a = app_a.connect_ws(&user_alice.token).await;

    let new_identity_key_bytes = common::generate_signing_key();
    let new_priv = PrivateKey(new_identity_key_bytes);
    let (_, new_ik_pub_ed) = new_priv.calculate_key_pair(0);
    let new_ik_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(new_ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut new_ik_pub_wire = new_ik_pub_mont.to_vec();
    new_ik_pub_wire.insert(0, 0x05);

    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&new_identity_key_bytes);

    let takeover_payload = json!({
        "identityKey": STANDARD.encode(new_ik_pub_wire),
        "registrationId": 999,
        "signedPreKey": {
            "keyId": 100,
            "publicKey": STANDARD.encode(&new_spk_pub),
            "signature": STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": []
    });

    let resp = app_b
        .client
        .post(format!("{}/v1/keys", app_b.server_url))
        .header("Authorization", format!("Bearer {}", user_alice.token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "Takeover failed: {}", resp.text().await.unwrap());

    let mut disconnected = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        match ws_a.receive_raw_timeout(Duration::from_millis(100)).await {
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | Some(Err(_)) | None => {
                disconnected = true;
                break;
            }
            _ => {}
        }
    }

    assert!(disconnected, "Alice was not disconnected from Node A after takeover on Node B");
}

#[tokio::test]
async fn test_distributed_fan_out_disconnect() {
    let config = common::get_test_config();
    let app_a = common::TestApp::spawn_with_config(config.clone()).await;
    let app_b = common::TestApp::spawn_with_config(config.clone()).await;
    let app_c = common::TestApp::spawn_with_config(config).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user_alice = app_a.register_user(&format!("alice_fanout_{}", run_id)).await;
    let user_bob = app_a.register_user(&format!("bob_fanout_{}", run_id)).await;

    let mut ws_a1 = app_a.connect_ws(&user_alice.token).await;
    let mut ws_b1 = app_b.connect_ws(&user_alice.token).await;
    let mut ws_b2 = app_b.connect_ws(&user_alice.token).await;
    let mut ws_bob = app_a.connect_ws(&user_bob.token).await;

    let new_ik = common::generate_signing_key();
    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&new_ik);
    let (_, ik_pub_ed) = PrivateKey(new_ik).calculate_key_pair(0);
    let mut new_ik_pub_wire = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed)
        .decompress()
        .unwrap()
        .to_montgomery()
        .to_bytes()
        .to_vec();
    new_ik_pub_wire.insert(0, 0x05);

    let takeover_payload = json!({
        "identityKey": STANDARD.encode(new_ik_pub_wire),
        "registrationId": 888,
        "signedPreKey": { "keyId": 200, "publicKey": STANDARD.encode(&new_spk_pub), "signature": STANDARD.encode(&new_spk_sig) },
        "oneTimePreKeys": []
    });

    let resp = app_c
        .client
        .post(format!("{}/v1/keys", app_c.server_url))
        .header("Authorization", format!("Bearer {}", user_alice.token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let mut sessions = [("A1", &mut ws_a1), ("B1", &mut ws_b1), ("B2", &mut ws_b2)];
    for (name, ws) in &mut sessions {
        let mut disconnected = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            match ws.receive_raw_timeout(Duration::from_millis(100)).await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | Some(Err(_)) | None => {
                    disconnected = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(disconnected, "Alice session {} was not disconnected", name);
    }

    ws_bob.sink.send(tokio_tungstenite::tungstenite::Message::Ping(vec![1].into())).await.unwrap();
    let pong = ws_bob.receive_pong().await;
    assert!(pong.is_some(), "Bob should have stayed connected");
}
