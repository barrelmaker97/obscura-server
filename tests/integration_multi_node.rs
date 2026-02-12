mod common;
use uuid::Uuid;

#[tokio::test]
async fn test_multi_node_notification() {
    // 1. Setup two instances of the app sharing the same backend (Valkey and Postgres)
    // The common::TestApp::spawn() uses environment variables or defaults that point to the same infra.
    let app_a = common::TestApp::spawn().await;
    let app_b = common::TestApp::spawn().await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let alice_name = format!("alice_{}", run_id);
    let bob_name = format!("bob_{}", run_id);

    // 2. Register Alice and Bob. Since they share the same DB, registration can happen on either node.
    let user_alice = app_a.register_user(&alice_name).await;
    let user_bob = app_b.register_user(&bob_name).await;

    // 3. Connect Bob to Node B via WebSocket
    let mut ws_bob = app_b.connect_ws(&user_bob.token).await;

    // 4. Alice sends a message to Bob via Node A
    let content = b"Cross-node message".to_vec();
    app_a.send_message(&user_alice.token, user_bob.user_id, &content).await;

    // 5. Bob should receive the message on Node B
    // This requires Node A to publish to Valkey and Node B to receive and route it to Bob's local broadcast channel.
    let env =
        ws_bob.receive_envelope().await.expect("Bob did not receive message on Node B via cross-node notification");
    let received_msg = env.message.expect("Envelope missing message");
    assert_eq!(received_msg.content, content);
}

#[tokio::test]
async fn test_multi_node_disconnect_notification() {
    let app_a = common::TestApp::spawn().await;
    let app_b = common::TestApp::spawn().await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let alice_name = format!("alice_takeover_{}", run_id);

    let user_alice = app_a.register_user(&alice_name).await;

    // 1. Alice connects to Node A
    let mut ws_a = app_a.connect_ws(&user_alice.token).await;

    // 2. Perform a takeover of Alice's account on Node B
    // This requires uploading a NEW identity key.
    use base64::Engine;
    use serde_json::json;
    use xeddsa::CalculateKeyPair;
    use xeddsa::xed25519::PrivateKey;

    let new_identity_key_bytes = common::generate_signing_key();
    let new_priv = PrivateKey(new_identity_key_bytes);
    let (_, new_ik_pub_ed) = new_priv.calculate_key_pair(0);
    let new_ik_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(new_ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut new_ik_pub_wire = new_ik_pub_mont.to_vec();
    new_ik_pub_wire.insert(0, 0x05);

    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&new_identity_key_bytes);

    let takeover_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(new_ik_pub_wire),
        "registrationId": 999,
        "signedPreKey": {
            "keyId": 100,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&new_spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": []
    });

    // Send takeover request to Node B
    let resp = app_b
        .client
        .post(format!("{}/v1/keys", app_b.server_url))
        .header("Authorization", format!("Bearer {}", user_alice.token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "Takeover on Node B failed");

    // 3. Alice on Node A should be disconnected
    // The server should send a Close frame or just drop the connection.
    // In our implementation (session.rs), it breaks the loop and closes the socket.

    let mut disconnected = false;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        match ws_a.receive_raw_timeout(std::time::Duration::from_millis(100)).await {
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                disconnected = true;
                break;
            }
            Some(Err(_)) | None => {
                disconnected = true;
                break;
            }
            _ => {
                // Ignore other messages (like the ACK for the takeover if any,
                // though takeover is REST, not WS)
            }
        }
    }

    assert!(disconnected, "Alice was not disconnected from Node A after takeover on Node B");
}
