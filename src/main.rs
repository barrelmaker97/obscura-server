use axum::{extract::Json, routing::post, Router, extract::State, serve};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use tokio::net::TcpListener;

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

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();

    serve(listener, app.into_make_service()).await.unwrap();

}

#[axum::debug_handler]
async fn upload_keys(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UploadKeysRequest>,
) -> &'static str {
    let mut keys = state.keys.lock().unwrap();
    keys.insert(req.user_id, req.keys);
    "OK"
}

#[axum::debug_handler]
async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendMessageRequest>,
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

#[axum::debug_handler]
async fn fetch_messages(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FetchMessagesRequest>,
) -> Json<Vec<Message>> {
    let mut messages = state.messages.lock().unwrap();
    let user_msgs: Vec<_> = messages
        .iter()
        .filter(|msg| msg.to == req.user_id)
        .cloned()
        .collect();

    messages.retain(|msg| msg.to != req.user_id);

    Json(user_msgs)
}

// Note: `drain_filter` requires nightly or a custom workaround for stable.
// Replace with `retain` and `clone` if you're using stable Rust.
