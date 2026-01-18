#![allow(dead_code)]
use obscura_server::{
    api::app_router,
    config::Config,
    core::notification::InMemoryNotifier,
    proto::obscura::v1::{
        web_socket_frame::Payload, AckMessage, EncryptedMessage, Envelope, WebSocketFrame,
    },
    storage,
};
use prost::Message as ProstMessage;
use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;
use std::sync::{Arc, Once};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream};
use uuid::Uuid;
use futures::{SinkExt, StreamExt};
use base64::Engine;

static INIT: Once = Once::new();

pub fn setup_tracing() {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "warn".into())
            .add_directive("obscura_server=debug".parse().unwrap())
            .add_directive("sqlx=warn".parse().unwrap())
            .add_directive("tower=warn".parse().unwrap())
            .add_directive("hyper=warn".parse().unwrap())
            .add_directive("reqwest=warn".parse().unwrap())
            .add_directive("rustls=warn".parse().unwrap())
            .add_directive("tungstenite=warn".parse().unwrap());

        tracing_subscriber::fmt().with_env_filter(filter).init();
    });
}

pub async fn get_test_pool() -> PgPool {
    setup_tracing();
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());

    let pool = storage::init_pool(&database_url)
        .await
        .expect("Failed to connect to DB. Is Postgres running?");

    // Run migrations automatically
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

#[allow(dead_code)]
pub fn get_test_config() -> Config {
    Config {
        database_url: "postgres://user:password@localhost/signal_server".to_string(),
        jwt_secret: "test_secret".to_string(),
        rate_limit_per_second: 10000,
        rate_limit_burst: 10000,
        auth_rate_limit_per_second: 10000,
        auth_rate_limit_burst: 10000,
        server_host: "127.0.0.1".to_string(),
        server_port: 0, // 0 means let OS choose
        message_ttl_days: 30,
        max_inbox_size: 1000,
        message_cleanup_interval_secs: 300,
        notification_gc_interval_secs: 60,
        notification_channel_capacity: 16,
        message_batch_limit: 50,
        trusted_proxies: "127.0.0.1/32,::1/128".to_string(),
        ws_outbound_buffer_size: 32,
        ws_ack_buffer_size: 100,
        ws_ack_batch_size: 50,
        ws_ack_flush_interval_ms: 500,
    }
}

#[allow(dead_code)]
pub struct TestApp {
    pub pool: PgPool,
    pub config: Config,
    pub server_url: String,
    pub ws_url: String,
    pub client: Client,
}

impl TestApp {
    pub async fn spawn() -> Self {
        Self::spawn_with_config(get_test_config()).await
    }

    pub async fn spawn_with_config(config: Config) -> Self {
        let pool = get_test_pool().await;
        let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
        let app = app_router(pool.clone(), config.clone(), notifier);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{}", addr);
        let ws_url = format!("ws://{}/v1/gateway", addr);

        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        TestApp {
            pool,
            config,
            server_url,
            ws_url,
            client: Client::new(),
        }
    }

    pub async fn register_user(&self, username: &str) -> (String, Uuid) {
        // Use a default registration ID if not specified by the test logic (most don't care)
        self.register_user_with_id(username, 123).await
    }

    pub async fn register_user_with_id(&self, username: &str, registration_id: u32) -> (String, Uuid) {
        let payload = json!({
            "username": username,
            "password": "password",
            "registrationId": registration_id,
            "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
            "signedPreKey": {
                "keyId": 1,
                "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
                "signature": "dGVzdF9zaWduZWRfc2ln"
            },
            "oneTimePreKeys": []
        });

        let resp = self
            .client
            .post(format!("{}/v1/accounts", self.server_url))
            .json(&payload)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 201, "User registration failed");
        
        let json: serde_json::Value = resp.json().await.unwrap();
        let token = json["token"].as_str().unwrap().to_string();
        
        // Decode user_id from token
        let parts: Vec<&str> = token.split('.').collect();
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        let user_id = Uuid::parse_str(claims["sub"].as_str().unwrap()).unwrap();

        (token, user_id)
    }

    pub async fn send_message(&self, token: &str, recipient_id: Uuid, content: &[u8]) {
        let enc_msg = EncryptedMessage {
            r#type: 2,
            content: content.to_vec(),
        };
        let mut buf = Vec::new();
        enc_msg.encode(&mut buf).unwrap();

        let resp = self
            .client
            .post(format!("{}/v1/messages/{}", self.server_url, recipient_id))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 201, "Message sending failed");
    }

    pub async fn connect_ws(&self, token: &str) -> TestWsClient {
        let (ws_stream, _) = connect_async(format!("{}?token={}", self.ws_url, token))
            .await
            .expect("Failed to connect WS");
        TestWsClient { stream: ws_stream }
    }
}

#[allow(dead_code)]
pub struct TestWsClient {
    pub stream: WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
}

impl TestWsClient {
    pub async fn receive_envelope(&mut self) -> Option<Envelope> {
        self.receive_envelope_timeout(std::time::Duration::from_secs(5)).await
    }

    pub async fn receive_envelope_timeout(&mut self, timeout: std::time::Duration) -> Option<Envelope> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
             match tokio::time::timeout(std::time::Duration::from_millis(100), self.stream.next()).await {
                Ok(Some(Ok(Message::Binary(bin)))) => {
                    let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
                    if let Some(Payload::Envelope(env)) = frame.payload {
                        return Some(env);
                    }
                }
                Ok(Some(Ok(Message::Close(_)))) => return None,
                _ => continue,
             }
        }
        None
    }

    pub async fn send_ack(&mut self, message_id: String) {
        let ack = AckMessage { message_id };
        let frame = WebSocketFrame {
            payload: Some(Payload::Ack(ack)),
        };
        let mut buf = Vec::new();
        frame.encode(&mut buf).unwrap();
        self.stream.send(Message::Binary(buf.into())).await.unwrap();
    }
    
    pub async fn close(self) {
        // Drop closes it
    }
}
