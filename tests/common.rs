#![allow(dead_code)]
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use obscura_server::{
    adapters,
    adapters::push::{PushError, PushProvider},
    api::app_router,
    config::{AuthConfig, Config, PubSubConfig, RateLimitConfig, ServerConfig, StorageConfig},
    proto::obscura::v1::{
        AckMessage, EncryptedMessage, Envelope, PreKeyStatus, WebSocketFrame, web_socket_frame::Payload,
    },
    services::notification_service::NotificationService,
};

use prost::Message as ProstMessage;
use rand::RngCore;
use rand::rngs::OsRng;
use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::protocol::Message};
use uuid::Uuid;
use xeddsa::xed25519::PrivateKey;
use xeddsa::{CalculateKeyPair, Sign};

static INIT: OnceLock<()> = OnceLock::new();

pub fn setup_tracing() {
    INIT.get_or_init(|| {
        obscura_server::telemetry::init_test_telemetry();
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "warn".into())
            .add_directive("tower=warn".parse().unwrap())
            .add_directive("sqlx=warn".parse().unwrap())
            .add_directive("hyper=warn".parse().unwrap())
            .add_directive("reqwest=warn".parse().unwrap())
            .add_directive("rustls=warn".parse().unwrap())
            .add_directive("tungstenite=warn".parse().unwrap())
            .add_directive("aws=warn".parse().unwrap());

        let format = std::env::var("OBSCURA_LOG_FORMAT").unwrap_or_else(|_| "text".to_string());
        use tracing_subscriber::fmt::format::FmtSpan;

        match format.as_str() {
            "json" => {
                tracing_subscriber::fmt().json().with_env_filter(filter).with_span_events(FmtSpan::CLOSE).init();
            }
            _ => {
                tracing_subscriber::fmt().with_env_filter(filter).init();
            }
        }
    });
}

/// Shared global state for all test providers to report to.
pub fn notification_counts() -> &'static DashMap<Uuid, u32> {
    static COUNTS: OnceLock<DashMap<Uuid, u32>> = OnceLock::new();
    COUNTS.get_or_init(DashMap::new)
}

#[derive(Debug, Default)]
pub struct SharedMockPushProvider;

#[async_trait]
impl PushProvider for SharedMockPushProvider {
    async fn send_push(&self, token: &str) -> Result<(), PushError> {
        if let Some(user_id_str) = token.strip_prefix("token:")
            && let Ok(user_id) = Uuid::parse_str(user_id_str)
        {
            *notification_counts().entry(user_id).or_insert(0) += 1;
        }
        Ok(())
    }
}

pub async fn get_test_pool() -> PgPool {
    setup_tracing();
    let database_url = std::env::var("OBSCURA_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());

    let pool =
        adapters::database::init_pool(&database_url).await.expect("Failed to connect to DB. Is Postgres running?");

    sqlx::migrate!().run(&pool).await.expect("Failed to run migrations");

    pool
}

pub async fn ensure_storage_bucket(s3_client: &aws_sdk_s3::Client, bucket: &str) {
    let _ = s3_client.create_bucket().bucket(bucket).send().await;
}

pub fn get_test_config() -> Config {
    let database_url = std::env::var("OBSCURA_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());

    let mut config = Config {
        database_url,
        ttl_days: 30,
        server: ServerConfig {
            port: 0,
            mgmt_port: 0,
            trusted_proxies: vec!["127.0.0.1/32".parse().unwrap(), "::1/128".parse().unwrap()],
            ..Default::default()
        },
        auth: AuthConfig { jwt_secret: "test_secret".to_string(), ..Default::default() },
        rate_limit: RateLimitConfig { per_second: 10000, burst: 10000, auth_per_second: 10000, auth_burst: 10000 },
        storage: StorageConfig {
            bucket: "test-bucket".to_string(),
            endpoint: Some(
                std::env::var("OBSCURA_STORAGE_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string()),
            ),
            access_key: Some("minioadmin".to_string()),
            secret_key: Some("minioadmin".to_string()),
            force_path_style: true,
            ..Default::default()
        },
        pubsub: PubSubConfig {
            url: std::env::var("OBSCURA_PUBSUB_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    // Test Isolation
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    config.notifications.push_queue_key = format!("jobs:test:{}", run_id);
    config.notifications.channel_prefix = format!("test:{}:", run_id);

    config
}

pub fn generate_signing_key() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn generate_signed_pre_key(identity_key_bytes: &[u8; 32]) -> (Vec<u8>, Vec<u8>) {
    // Generate SPK
    let spk_bytes = generate_signing_key();
    let spk_priv = PrivateKey(spk_bytes);
    let (_, spk_pub_ed) = spk_priv.calculate_key_pair(0);
    // Convert to Montgomery and add prefix
    let spk_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(spk_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut spk_pub_wire = [0u8; 33];
    spk_pub_wire[0] = 0x05;
    spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);

    // Sign SPK pub key (33-byte wire format) with Identity Key
    let ik_priv = PrivateKey(*identity_key_bytes);
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_wire, OsRng);

    (spk_pub_wire.to_vec(), signature.to_vec())
}

pub fn generate_registration_payload(
    username: &str,
    password: &str,
    reg_id: u32,
    otpk_count: usize,
) -> (serde_json::Value, [u8; 32]) {
    let identity_key = generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont =
        curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = [0u8; 33];
    ik_pub_wire[0] = 0x05;
    ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);

    let (spk_pub, spk_sig) = generate_signed_pre_key(&identity_key);

    let mut otpk = Vec::new();
    for i in 0..otpk_count {
        let key_bytes = generate_signing_key();
        let key_priv = PrivateKey(key_bytes);
        let (_, key_pub_ed) = key_priv.calculate_key_pair(0);
        let key_pub_mont =
            curve25519_dalek::edwards::CompressedEdwardsY(key_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
        let mut key_pub_wire = [0u8; 33];
        key_pub_wire[0] = 0x05;
        key_pub_wire[1..].copy_from_slice(&key_pub_mont);

        otpk.push(json!({
            "keyId": i,
            "publicKey": STANDARD.encode(key_pub_wire)
        }));
    }

    let payload = json!({
        "username": username,
        "password": password,
        "registrationId": reg_id,
        "identityKey": STANDARD.encode(ik_pub_wire),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(&spk_pub),
            "signature": STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": otpk
    });

    (payload, identity_key)
}

pub struct TestUser {
    pub user_id: Uuid,
    pub token: String,
    pub refresh_token: String,
    pub identity_key: [u8; 32],
}

pub struct TestApp {
    pub pool: PgPool,
    pub config: Config,
    pub server_url: String,
    pub mgmt_url: String,
    pub ws_url: String,
    pub client: Client,
    pub s3_client: aws_sdk_s3::Client,
    pub notifier: NotificationService,
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl TestApp {
    pub async fn spawn() -> Self {
        Self::spawn_with_config(get_test_config()).await
    }

    pub async fn spawn_with_config(config: Config) -> Self {
        let pool = get_test_pool().await;
        let mut config = config;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        config.server.port = addr.port();

        let mgmt_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mgmt_addr = mgmt_listener.local_addr().unwrap();
        config.server.mgmt_port = mgmt_addr.port();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let pubsub = adapters::redis::RedisClient::new(
            &config.pubsub,
            config.notifications.global_channel_capacity,
            shutdown_rx.clone(),
        )
        .await
        .expect("Failed to create RedisClient for tests. Is Redis running?");

        let s3_client = obscura_server::init_s3_client(&config.storage).await;

        let push_provider = Arc::new(SharedMockPushProvider);
        let app = obscura_server::AppBuilder::new(config.clone())
            .with_database(pool.clone())
            .with_pubsub(pubsub.clone())
            .with_s3(s3_client.clone())
            .with_push_provider(push_provider)
            .with_shutdown_rx(shutdown_rx.clone())
            .build()
            .await
            .expect("Failed to build application for tests");

        // Spawn workers explicitly in tests if needed (some tests might want to control this)
        let _worker_tasks = app.workers.spawn_all(shutdown_rx.clone());

        let notifier = app.services.notification_service.clone();
        let app_router = app_router(config.clone(), app.services, shutdown_rx.clone());
        let mgmt_app =
            obscura_server::api::mgmt_router(obscura_server::api::MgmtState { health_service: app.health_service });

        let server_url = format!("http://{}", addr);
        let mgmt_url = format!("http://{}", mgmt_addr);
        let ws_url = format!("ws://{}/v1/gateway", addr);

        tokio::spawn(async move {
            axum::serve(listener, app_router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
                .unwrap();
        });

        tokio::spawn(async move {
            axum::serve(mgmt_listener, mgmt_app.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
                .unwrap();
        });

        TestApp { pool, config, server_url, mgmt_url, ws_url, client: Client::new(), s3_client, notifier, shutdown_tx }
    }

    pub async fn register_user(&self, username: &str) -> TestUser {
        self.register_user_with_keys(username, 123, 1).await
    }

    pub async fn register_user_with_keys(&self, username: &str, reg_id: u32, otpk_count: usize) -> TestUser {
        let (reg_payload, identity_key) = generate_registration_payload(username, "password12345", reg_id, otpk_count);

        let resp = self.client.post(format!("{}/v1/users", self.server_url)).json(&reg_payload).send().await.unwrap();

        assert_eq!(resp.status(), 201, "User registration failed: {}", resp.text().await.unwrap());
        let body = resp.json::<serde_json::Value>().await.unwrap();
        let token = body["token"].as_str().unwrap().to_string();
        let refresh_token = body["refreshToken"].as_str().unwrap_or_default().to_string();

        let parts: Vec<&str> = token.split('.').collect();
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        let user_id = Uuid::parse_str(claims["sub"].as_str().unwrap()).unwrap();

        TestUser { user_id, token, refresh_token, identity_key }
    }

    pub async fn send_message(&self, token: &str, recipient_id: Uuid, content: &[u8]) {
        let enc_msg = EncryptedMessage { r#type: 2, content: content.to_vec() };
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
        let (ws_stream, _) =
            connect_async(format!("{}?token={}", self.ws_url, token)).await.expect("Failed to connect WS");
        let (sink, stream) = ws_stream.split();
        let (tx_env, rx_env) = tokio::sync::mpsc::unbounded_channel();
        let (tx_status, rx_status) = tokio::sync::mpsc::unbounded_channel();
        let (tx_pong, rx_pong) = tokio::sync::mpsc::unbounded_channel();
        let (tx_raw, rx_raw) = tokio::sync::mpsc::unbounded_channel();

        let mut stream = stream;
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                if let Ok(m) = &msg {
                    let _ = tx_raw.send(Ok(m.clone()));
                }
                match msg {
                    Ok(Message::Binary(bin)) => {
                        if let Ok(frame) = WebSocketFrame::decode(bin.as_ref()) {
                            match frame.payload {
                                Some(Payload::Envelope(e)) => {
                                    let _ = tx_env.send(e);
                                }
                                Some(Payload::PreKeyStatus(s)) => {
                                    let _ = tx_status.send(s);
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Message::Pong(p)) => {
                        let _ = tx_pong.send(p);
                    }
                    _ => {}
                }
            }
        });

        TestWsClient { sink, rx_env, rx_status, rx_pong, rx_raw }
    }

    pub async fn wait_until<F, Fut>(&self, mut condition: F, timeout: Duration) -> bool
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if condition().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    pub async fn assert_message_count(&self, user_id: Uuid, expected: i64) {
        let success = self
            .wait_until(
                || async {
                    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
                        .bind(user_id)
                        .fetch_one(&self.pool)
                        .await
                        .unwrap();
                    count == expected
                },
                Duration::from_secs(5),
            )
            .await;

        if !success {
            let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .unwrap();
            panic!("Message count assertion failed. Expected {}, got {}", expected, actual);
        }
    }
}

pub struct TestWsClient {
    pub sink:
        futures::stream::SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>,
    pub rx_env: tokio::sync::mpsc::UnboundedReceiver<Envelope>,
    pub rx_status: tokio::sync::mpsc::UnboundedReceiver<PreKeyStatus>,
    pub rx_pong: tokio::sync::mpsc::UnboundedReceiver<tokio_tungstenite::tungstenite::Bytes>,
    pub rx_raw: tokio::sync::mpsc::UnboundedReceiver<Result<Message, tokio_tungstenite::tungstenite::Error>>,
}

impl TestWsClient {
    pub async fn receive_pong(&mut self) -> Option<Vec<u8>> {
        tokio::time::timeout(Duration::from_secs(5), self.rx_pong.recv()).await.ok().flatten().map(|b| b.to_vec())
    }

    pub async fn receive_envelope(&mut self) -> Option<Envelope> {
        self.receive_envelope_timeout(Duration::from_secs(5)).await
    }

    pub async fn receive_envelope_timeout(&mut self, timeout: Duration) -> Option<Envelope> {
        tokio::time::timeout(timeout, self.rx_env.recv()).await.ok().flatten()
    }

    pub async fn receive_prekey_status(&mut self) -> Option<PreKeyStatus> {
        self.receive_prekey_status_timeout(Duration::from_secs(5)).await
    }

    pub async fn receive_prekey_status_timeout(&mut self, timeout: Duration) -> Option<PreKeyStatus> {
        tokio::time::timeout(timeout, self.rx_status.recv()).await.ok().flatten()
    }

    pub async fn receive_raw_timeout(
        &mut self,
        timeout: Duration,
    ) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
        tokio::time::timeout(timeout, self.rx_raw.recv()).await.ok().flatten()
    }

    pub async fn send_ack(&mut self, message_id: String) {
        let ack = AckMessage { message_id };
        let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
        let mut buf = Vec::new();
        frame.encode(&mut buf).unwrap();
        self.sink.send(Message::Binary(buf.into())).await.unwrap();
    }
}
