#![allow(dead_code)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use obscura_server::{
    api::app_router,
    config::Config,
    core::notification::InMemoryNotifier,
    proto::obscura::v1::{
        AckMessage, EncryptedMessage, Envelope, PreKeyStatus, WebSocketFrame, web_socket_frame::Payload,
    },
    storage,
};
use prost::Message as ProstMessage;
use rand::RngCore;
use rand::rngs::OsRng;
use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;
use std::sync::{Arc, Once};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::protocol::Message};
use uuid::Uuid;
use xeddsa::xed25519::PrivateKey;
use xeddsa::{CalculateKeyPair, Sign};

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
    let database_url = std::env::var("OBSCURA_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());

    let pool = storage::init_pool(&database_url).await.expect("Failed to connect to DB. Is Postgres running?");

    sqlx::migrate!().run(&pool).await.expect("Failed to run migrations");

    pool
}

pub fn get_test_config() -> Config {
    Config {
        database_url: "postgres://user:password@localhost/signal_server".to_string(),
        ttl_days: 30,
        server: obscura_server::config::ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            trusted_proxies: vec!["127.0.0.1/32".parse().unwrap(), "::1/128".parse().unwrap()],
        },
        auth: obscura_server::config::AuthConfig {
            jwt_secret: "test_secret".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_days: 30,
        },
        rate_limit: obscura_server::config::RateLimitConfig {
            per_second: 10000,
            burst: 10000,
            auth_per_second: 10000,
            auth_burst: 10000,
        },
        messaging: obscura_server::config::MessagingConfig {
            max_inbox_size: 1000,
            cleanup_interval_secs: 300,
            batch_limit: 50,
            pre_key_refill_threshold: 20,
            max_pre_keys: 100,
        },
        notifications: obscura_server::config::NotificationConfig { gc_interval_secs: 60, channel_capacity: 16 },
        websocket: obscura_server::config::WsConfig {
            outbound_buffer_size: 32,
            ack_buffer_size: 100,
            ack_batch_size: 50,
            ack_flush_interval_ms: 500,
        },
        s3: obscura_server::config::S3Config {
            bucket: "test-bucket".to_string(),
            region: "us-east-1".to_string(),
            endpoint: Some("http://localhost:9000".to_string()),
            access_key: Some("minioadmin".to_string()),
            secret_key: Some("minioadmin".to_string()),
            force_path_style: true,
            attachment_max_size_bytes: 52_428_800,
        },
    }
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
    let spk_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(spk_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut spk_pub_wire = [0u8; 33];
    spk_pub_wire[0] = 0x05;
    spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);

    // Sign SPK pub key (33-byte wire format) with Identity Key
    let ik_priv = PrivateKey(*identity_key_bytes);
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_wire, OsRng);

    (spk_pub_wire.to_vec(), signature.to_vec())
}

pub fn generate_registration_payload(username: &str, password: &str, reg_id: u32, otpk_count: usize) -> (serde_json::Value, [u8; 32]) {
    let identity_key = generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = [0u8; 33];
    ik_pub_wire[0] = 0x05;
    ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);
    
    let (spk_pub, spk_sig) = generate_signed_pre_key(&identity_key);

    let mut otpk = Vec::new();
    for i in 0..otpk_count {
        let key_bytes = generate_signing_key();
        let key_priv = PrivateKey(key_bytes);
        let (_, key_pub_ed) = key_priv.calculate_key_pair(0);
        let key_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(key_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
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
    pub ws_url: String,
    pub client: Client,
    pub s3_client: aws_sdk_s3::Client,
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

        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let notifier = Arc::new(InMemoryNotifier::new(config.clone(), shutdown_rx));

        let region_provider = aws_config::Region::new(config.s3.region.clone());
        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region_provider);

        if let Some(ref endpoint) = config.s3.endpoint {
            config_loader = config_loader.endpoint_url(endpoint);
        }

        if let (Some(ak), Some(sk)) = (&config.s3.access_key, &config.s3.secret_key) {
            let creds = aws_credential_types::Credentials::new(ak.clone(), sk.clone(), None, None, "static");
            config_loader = config_loader.credentials_provider(creds);
        }

        let sdk_config = config_loader.load().await;
        let s3_config_builder =
            aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(config.s3.force_path_style);
        let s3_client = aws_sdk_s3::Client::from_conf(s3_config_builder.build());

        let app = app_router(pool.clone(), config.clone(), notifier, s3_client.clone());

        let server_url = format!("http://{}", addr);
        let ws_url = format!("ws://{}/v1/gateway", addr);

        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
        });

        TestApp { pool, config, server_url, ws_url, client: Client::new(), s3_client }
    }

    pub async fn register_user(&self, username: &str) -> TestUser {
        self.register_user_with_keys(username, 123, 1).await
    }

    pub async fn register_user_with_keys(&self, username: &str, reg_id: u32, otpk_count: usize) -> TestUser {
        let (reg_payload, identity_key) = generate_registration_payload(username, "password", reg_id, otpk_count);

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

        let mut stream = stream;
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
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

        TestWsClient { sink, rx_env, rx_status, rx_pong }
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

    pub async fn send_ack(&mut self, message_id: String) {
        let ack = AckMessage { message_id };
        let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
        let mut buf = Vec::new();
        frame.encode(&mut buf).unwrap();
        self.sink.send(Message::Binary(buf.into())).await.unwrap();
    }
}
