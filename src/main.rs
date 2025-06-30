// Minimal Signal-like Server Prototype in Rust
// Dependencies (add to Cargo.toml):
// axum = "0.7"
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"
// uuid = { version = "1", features = ["serde", "v4"] }
// tokio = { version = "1", features = ["macros", "rt-multi-thread"] }

use axum::{extract::Json, routing::post, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// In-memory DB types
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceKeys {
    identity_key: String,
    signed_prekey: String,
    prekey_signature: String,
    one_time_prekeys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    from: Uuid,
    to: Uuid,
    ciphertext: String,
}

#[derive(Default)]
struct AppState {
    keys: Mutex<HashMap<Uuid, DeviceKeys>>, // user_id -> keys
    messages: Mutex<Vec<Message>>,          // naive message queue
}

#[derive(Debug, Deserialize)]
struct UploadKeysRequest {
    user_id: Uuid,
    keys: DeviceKeys,
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    from: Uuid,
    to: Uuid,
    ciphertext: String,
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::default());

    let app = Router::new()
        .route("/upload_keys", post(upload_keys))
        .route("/send_message", post(send_message))
        .route("/fetch_messages", post(fetch_messages))
        .with_state(state);

    println!("Running on http://localhost:3000");
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn upload_keys(
    Json(req): Json<UploadKeysRequest>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> &'static str {
    let mut keys = state.keys.lock().unwrap();
    keys.insert(req.user_id, req.keys);
    "OK"
}

async fn send_message(
    Json(req): Json<SendMessageRequest>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> &'static str {
    let mut messages = state.messages.lock().unwrap();
    messages.push(Message {
        from: req.from,
        to: req.to,
        ciphertext: req.ciphertext,
    });
    "OK"
}

#[derive(Debug, Deserialize)]
struct FetchMessagesRequest {
    user_id: Uuid,
}

async fn fetch_messages(
    Json(req): Json<FetchMessagesRequest>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Json<Vec<Message>> {
    let mut messages = state.messages.lock().unwrap();
    let user_msgs: Vec<_> = messages
        .drain_filter(|msg| msg.to == req.user_id)
        .collect();
    Json(user_msgs)
}

// Note: `drain_filter` requires nightly or a custom workaround for stable.
// Replace with `retain` and `clone` if you're using stable Rust.
