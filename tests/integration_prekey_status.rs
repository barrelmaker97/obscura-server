use futures::StreamExt;
use obscura_server::proto::obscura::v1::{WebSocketFrame, web_socket_frame::Payload};
use prost::Message as ProstMessage;
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message;
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
    let msg = ws.stream.next().await.expect("Stream closed unexpectedly").expect("Error receiving message");
    
    if let Message::Binary(bin) = msg {
        let frame = WebSocketFrame::decode(bin.as_ref()).expect("Failed to decode frame");
        if let Some(Payload::PreKeyStatus(status)) = frame.payload {
            assert_eq!(status.one_time_pre_key_count, 0);
            assert_eq!(status.min_threshold, 20);
        } else {
            panic!("Expected PreKeyStatus, got {:?}", frame.payload);
        }
    } else {
        panic!("Expected Binary message");
    }
}

#[tokio::test]
async fn test_prekey_status_sufficient_keys() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("status_user_ok_{}", run_id);

    // 1. Register with 25 one-time keys (above threshold of 20)
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
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": keys
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Connect WebSocket
    let mut ws = app.connect_ws(&token).await;

    // 3. Expect NO PreKeyStatus message
    // We wait a bit to see if anything comes. 
    // Since there are no pending messages, the stream should be idle.
    // If we receive something, it's an error (unless it's a ping, but we don't have pings yet).
    let timeout = std::time::Duration::from_millis(500);
    match tokio::time::timeout(timeout, ws.stream.next()).await {
        Ok(Some(Ok(Message::Binary(bin)))) => {
             let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
             if let Some(Payload::PreKeyStatus(_)) = frame.payload {
                 panic!("Received PreKeyStatus unexpectedly!");
             }
        }
        Ok(Some(Ok(Message::Close(_)))) => {}, // Close is fine
        Ok(None) => {}, // Stream end is fine
        Ok(Some(Err(_))) => {},
        Ok(Some(Ok(_))) => {}, // Other messages ignored
        Err(_) => {}, // Timeout is SUCCESS here (no message received)
    }
}
