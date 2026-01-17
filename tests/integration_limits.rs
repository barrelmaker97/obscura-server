use base64::Engine;
use futures::StreamExt;
use obscura_server::proto::obscura::v1::{EncryptedMessage, WebSocketFrame, web_socket_frame::Payload};
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

    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;

    let user_b_name = format!("bob_{}", run_id);
    let token_b = register_user(&client, &server_url, &user_b_name, 2).await;
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    for i in 0..1005 {
        let payload = format!("msg_{}", i).into_bytes();
                let enc_msg = EncryptedMessage {
                r#type: 2, // ENCRYPTED_MESSAGE
                content: payload,
            };        let mut buf = Vec::new();
        enc_msg.encode(&mut buf).unwrap();

        let resp = client
            .post(format!("{}/v1/messages/{}", server_url, user_b_id))
            .header("Authorization", format!("Bearer {}", token_a))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "Failed to send message {} during flooding", i);
    }

    let repo = MessageRepository::new(pool.clone());
    repo.delete_global_overflow(1000).await.expect("Failed to run cleanup");

    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    if let Some(msg) = ws_stream.next().await {
        let msg = msg.unwrap();
        if let Message::Binary(bin) = msg {
            let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
            if let Some(Payload::Envelope(env)) = frame.payload {
                let content = env.message.unwrap().content;
                assert_eq!(content, b"msg_5", "First message should be msg_5 (0-4 should have been pruned)");
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

    let req1 = Request::builder()
        .uri("/v1/gateway?token=bad")
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_ne!(resp1.status(), StatusCode::TOO_MANY_REQUESTS);

    let req2 = Request::builder()
        .uri("/v1/gateway?token=bad")
        .extension(axum::extract::connect_info::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::empty())
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
}

async fn register_user(client: &reqwest::Client, server_url: &str, username: &str, reg_id: u32) -> String {
    let reg = json!({
        "username": username,
        "password": "password",
        "registrationId": reg_id,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });
    let resp = client.post(format!("{}/v1/accounts", server_url)).json(&reg).send().await.unwrap();
    assert_eq!(resp.status(), 201, "User registration failed in limits test");
    resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string()
}
