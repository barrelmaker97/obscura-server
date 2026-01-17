use base64::Engine;
use futures::StreamExt;
use obscura_server::proto::obscura::v1::{OutgoingMessage, WebSocketFrame, web_socket_frame::Payload};
use obscura_server::{api::app_router, core::notification::InMemoryNotifier, storage::message_repo::MessageRepository};
use prost::Message as ProstMessage;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_message_limit_fifo() {
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();

    // Clear messages table to ensure clean state for this test if reuse happens
    sqlx::query("DELETE FROM messages").execute(&pool).await.unwrap();

    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool.clone(), config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);
    let ws_url = format!("ws://{}/v1/gateway", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
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

    let resp_a = client.post(format!("{}/v1/accounts", server_url)).json(&reg_a).send().await.unwrap();
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

    let resp_b = client.post(format!("{}/v1/accounts", server_url)).json(&reg_b).send().await.unwrap();

    let token_b = resp_b.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 4. Send 1005 Messages from A to B
    // We expect 5 oldest to be dropped, leaving 1000.
    // Messages 0, 1, 2, 3, 4 should be gone. Message 5 should be the first one.
    for i in 0..1005 {
        let payload = format!("msg_{}", i).into_bytes();
        let outgoing = OutgoingMessage { r#type: 1, content: payload };
        let mut buf = Vec::new();
        outgoing.encode(&mut buf).unwrap();

        client
            .post(format!("{}/v1/messages/{}", server_url, user_b_id))
            .header("Authorization", format!("Bearer {}", token_a))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .await
            .unwrap();
    }

    // 5. Trigger Background Cleanup Manually
    // Since we moved cleanup to background, we must simulate the job running now.
    let repo = MessageRepository::new(pool.clone());
    repo.delete_global_overflow(1000).await.expect("Failed to run cleanup");

    // 6. Connect User B and Verify
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

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

#[tokio::test]
async fn test_rate_limiting() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    let pool = common::get_test_pool().await;
    let mut config = common::get_test_config();
    config.rate_limit_per_second = 1;
    config.rate_limit_burst = 1;

    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool, config.clone(), notifier);

    // 2. First Request - Should Pass (or at least not be Rate Limited)
    // We hit a non-existent endpoint or just check auth failure, doesn't matter.
    // Rate limit layer runs before auth.

    // We need to provide ConnectInfo for the rate limiter to work in tests.
    // However, axum's `oneshot` doesn't easily inject ConnectInfo unless we wrap the app or mock it.
    // tower-governor by default uses PeerIp.
    // When using `oneshot`, there is no TCP connection, so no IP.
    // tower-governor usually falls back or allows if it can't extract key?
    // OR we need to use `ConnectInfo` extension manually.

    // Let's try sending it. If tower-governor fails to extract IP, it might panic or allow.
    // Ideally we want to verify it blocks.
    // To mock ConnectInfo in tests:
    let req1 = Request::builder()
        .uri("/v1/gateway?token=bad")
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_ne!(resp1.status(), StatusCode::TOO_MANY_REQUESTS);

    // 3. Second Request - Should Fail (Burst exceeded)
    let req2 = Request::builder()
        .uri("/v1/gateway?token=bad")
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::empty())
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
}
