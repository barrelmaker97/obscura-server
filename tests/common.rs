#![allow(dead_code)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use obscura_server::{
    api::{ServiceContainer, app_router},
    config::Config,
    proto::obscura::v1::{
        AckMessage, EncryptedMessage, Envelope, PreKeyStatus, WebSocketFrame, web_socket_frame::Payload,
    },
    services::{
        account_service::AccountService,
        attachment_service::AttachmentService,
        gateway::GatewayService,
        health_service::HealthService,
        key_service::KeyService,
        message_service::MessageService,
        notification_service::{DistributedNotificationService, NotificationService},
        rate_limit_service::RateLimitService,
    },
    storage::{
        self, attachment_repo::AttachmentRepository, key_repo::KeyRepository, message_repo::MessageRepository,
        refresh_token_repo::RefreshTokenRepository, user_repo::UserRepository,
    },
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

pub async fn get_test_pool() -> PgPool {
    setup_tracing();
    let database_url = std::env::var("OBSCURA_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string());

    let pool = storage::init_pool(&database_url).await.expect("Failed to connect to DB. Is Postgres running?");

    sqlx::migrate!().run(&pool).await.expect("Failed to run migrations");

    pool
}

pub async fn ensure_storage_bucket(s3_client: &aws_sdk_s3::Client, bucket: &str) {
    let _ = s3_client.create_bucket().bucket(bucket).send().await;
}

pub fn get_test_config() -> Config {
    Config {
        database_url: std::env::var("OBSCURA_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://user:password@localhost/signal_server".to_string()),
        server: obscura_server::config::ServerConfig {
            port: 0,
            mgmt_port: 0,
            trusted_proxies: vec!["127.0.0.1/32".parse().unwrap(), "::1/128".parse().unwrap()],
            ..Default::default()
        },
        auth: obscura_server::config::AuthConfig { jwt_secret: "test_secret".to_string(), ..Default::default() },
        rate_limit: obscura_server::config::RateLimitConfig {
            per_second: 10000,
            burst: 10000,
            auth_per_second: 10000,
            auth_burst: 10000,
        },
        storage: obscura_server::config::StorageConfig {
            bucket: "test-bucket".to_string(),
            endpoint: Some(
                std::env::var("OBSCURA_STORAGE_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string()),
            ),
            access_key: Some("minioadmin".to_string()),
            secret_key: Some("minioadmin".to_string()),
            force_path_style: true,
            ..Default::default()
        },
        pubsub: obscura_server::config::PubSubConfig {
            url: std::env::var("OBSCURA_PUBSUB_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            ..Default::default()
        },
        ..Default::default()
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

        let pubsub = storage::redis::RedisClient::new(
            &config.pubsub,
            config.notifications.global_channel_capacity,
            shutdown_rx.clone(),
        )
        .await
        .expect("Failed to create RedisClient for tests. Is Redis running?");

        let notifier: Arc<dyn NotificationService> = Arc::new(
            DistributedNotificationService::new(pubsub.clone(), &config, shutdown_rx.clone())
                .await
                .expect("Failed to create DistributedNotificationService for tests."),
        );

        let region_provider = aws_config::Region::new(config.storage.region.clone());
        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region_provider);

        if let Some(ref endpoint) = config.storage.endpoint {
            config_loader = config_loader.endpoint_url(endpoint);
        }

        if let (Some(ak), Some(sk)) = (&config.storage.access_key, &config.storage.secret_key) {
            let creds = aws_credential_types::Credentials::new(ak.clone(), sk.clone(), None, None, "static");
            config_loader = config_loader.credentials_provider(creds);
        }

        let sdk_config = config_loader.load().await;
        let s3_config_builder =
            aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(config.storage.force_path_style);
        let s3_client = aws_sdk_s3::Client::from_conf(s3_config_builder.build());

        // Initialize Repositories
        let key_repo = KeyRepository::new();
        let message_repo = MessageRepository::new();
        let user_repo = UserRepository::new();
        let refresh_repo = RefreshTokenRepository::new();
        let attachment_repo = AttachmentRepository::new();

        let crypto_service = obscura_server::services::crypto_service::CryptoService::new();

        // Initialize Services
        let key_service = KeyService::new(pool.clone(), key_repo, crypto_service.clone(), config.messaging.clone());

        let attachment_service = AttachmentService::new(
            pool.clone(),
            attachment_repo,
            s3_client.clone(),
            config.storage.clone(),
            config.ttl_days,
        );

        let auth_service = obscura_server::services::auth_service::AuthService::new(
            config.auth.clone(),
            pool.clone(),
            user_repo.clone(),
            refresh_repo.clone(),
        );

        let message_service = MessageService::new(
            pool.clone(),
            message_repo.clone(),
            notifier.clone(),
            config.messaging.clone(),
            config.ttl_days,
        );

        let account_service = AccountService::new(
            pool.clone(),
            user_repo,
            message_repo.clone(),
            auth_service.clone(),
            key_service.clone(),
            notifier.clone(),
        );

        let gateway_service = GatewayService::new(
            message_service.clone(),
            key_service.clone(),
            notifier.clone(),
            config.websocket.clone(),
        );

        let rate_limit_service = RateLimitService::new(config.server.trusted_proxies.clone());

        let health_service = HealthService::new(
            pool.clone(),
            s3_client.clone(),
            pubsub.clone(),
            config.storage.bucket.clone(),
            config.health.clone(),
        );

        let services = ServiceContainer {
            pool: pool.clone(),
            key_service,
            attachment_service,
            account_service,
            auth_service,
            message_service,
            gateway_service,
            rate_limit_service,
        };

        let app = app_router(config.clone(), services, shutdown_rx.clone());

        let mgmt_state = obscura_server::api::MgmtState { health_service };
        let mgmt_app = obscura_server::api::mgmt_router(mgmt_state);

        let server_url = format!("http://{}", addr);
        let mgmt_url = format!("http://{}", mgmt_addr);
        let ws_url = format!("ws://{}/v1/gateway", addr);

        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
        });

        tokio::spawn(async move {
            axum::serve(mgmt_listener, mgmt_app.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
                .unwrap();
        });

        TestApp { pool, config, server_url, mgmt_url, ws_url, client: Client::new(), s3_client, shutdown_tx }
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
