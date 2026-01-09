use tokio::net::TcpListener;
use obscura_server::{api::app_router, config::Config, storage};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures::StreamExt;
use serde_json::json;
use uuid::Uuid;
use obscura_server::proto::obscura::v1::{WebSocketFrame, OutgoingMessage, web_socket_frame::Payload};
use prost::Message as ProstMessage; 
use base64::Engine;

#[tokio::test]
async fn test_message_limit_fifo() {
    // 1. Setup Server
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());
    
    let config = Config {
        database_url: database_url.clone(),
        jwt_secret: "test_secret".to_string(),
    };

    let pool = storage::init_pool(&config.database_url).await.expect("Failed to connect to DB");
    // Clear messages table to ensure clean state for this test if reuse happens
    sqlx::query("DELETE FROM messages").execute(&pool).await.unwrap();

    let app = app_router(pool.clone(), config);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);
    let ws_url = format!("ws://{}/v1/gateway", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 2. Register User A (Sender)
    let user_a_name = format!("alice_{}", run_id);
    let reg_a = json!({
        "username": user_a_name,
        "password": "password",
        "registrationId": 1,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=", 
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });

    let resp_a = client.post(format!("{}/v1/accounts", server_url))
        .json(&reg_a)
        .send()
        .await
        .unwrap();
    let token_a = resp_a.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 3. Register User B (Recipient)
    let user_b_name = format!("bob_{}", run_id);
    let reg_b = json!({
        "username": user_b_name,
        "password": "password",
        "registrationId": 2,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=", 
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });

    let resp_b = client.post(format!("{}/v1/accounts", server_url))
        .json(&reg_b)
        .send()
        .await
        .unwrap();
    
    let token_b = resp_b.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 4. Send 1005 Messages from A to B
    // We expect 5 oldest to be dropped, leaving 1000.
    // Messages 0, 1, 2, 3, 4 should be gone. Message 5 should be the first one.
    for i in 0..1005 {
        let payload = format!("msg_{}", i).into_bytes();
        let outgoing = OutgoingMessage {
            r#type: 1,
            content: payload,
            timestamp: 123456789,
        };
        let mut buf = Vec::new();
        outgoing.encode(&mut buf).unwrap();

        client.post(format!("{}/v1/messages/{}", server_url, user_b_id))
            .header("Authorization", format!("Bearer {}", token_a))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .await
            .unwrap();
    }

    // 5. Connect User B and Verify
    let (mut ws_stream, _) = connect_async(format!("{}?token={}", ws_url, token_b))
        .await
        .expect("Failed to connect WS");

    // Receive first message
    if let Some(msg) = ws_stream.next().await {
        let msg = msg.unwrap();
        if let Message::Binary(bin) = msg {
            let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
            if let Some(Payload::Envelope(env)) = frame.payload {
                // The oldest available message should be "msg_5"
                assert_eq!(env.content, b"msg_5");
            }
        }
    } else {
        panic!("No messages received");
    }
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
