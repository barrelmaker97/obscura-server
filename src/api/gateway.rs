use crate::api::AppState;
use crate::api::middleware::verify_jwt;
use crate::core::notification::UserEvent;
use crate::proto::obscura::v1::{IncomingEnvelope, OutgoingMessage, WebSocketFrame, web_socket_frame::Payload};
use crate::storage::key_repo::KeyRepository;
use crate::storage::message_repo::MessageRepository;
use axum::{
    extract::{
        Query, State,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct WsParams {
    token: String,
}

use std::collections::HashSet;

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match verify_jwt(&params.token, &state.config.jwt_secret) {
        Ok(claims) => ws.on_upgrade(move |socket| handle_socket(socket, state, claims.sub)),
        Err(_) => axum::http::StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn handle_socket(mut socket: WebSocket, state: AppState, user_id: Uuid) {
    let repo = MessageRepository::new(state.pool.clone());
    let key_repo = KeyRepository::new(state.pool.clone());
    let mut sent_message_ids = HashSet::new();

    // Check for Identity Key
    if let Ok(None) = key_repo.fetch_identity_key(user_id).await {
        // No identity key found, close connection
        let _ = socket.close().await;
        return;
    }

    let mut rx = state.notifier.subscribe(user_id);

    // Initial check for pending messages on connect
    if !flush_messages(&mut socket, &repo, user_id, &mut sent_message_ids).await {
        return;
    }

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(WsMessage::Binary(bin))) => {
                         if let Ok(frame) = WebSocketFrame::decode(bin.as_ref())
                             && let Some(Payload::Ack(ack)) = frame.payload
                             && let Ok(msg_id) = Uuid::parse_str(&ack.message_id) {
                                 if repo.delete(msg_id).await.is_ok() {
                                     sent_message_ids.remove(&ack.message_id);
                                 }
                         }
                    }
                    Some(Ok(WsMessage::Close(_))) => break,
                    Some(Err(_)) => break,
                    None => break,
                    _ => {}
                }
            }
            // Event-driven trigger
            result = rx.recv() => {
                let mut should_fetch = match result {
                    Ok(event) => {
                        match event {
                            UserEvent::MessageReceived => true,
                            UserEvent::Disconnect => break,
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                         // Lagged: Assume message received
                         true
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                };

                // Drain any pending notifications to avoid redundant DB checks
                // If we see a Disconnect while draining, we should break immediately
                let mut disconnect_seen = false;
                while let Ok(evt) = rx.try_recv() {
                    match evt {
                        UserEvent::Disconnect => disconnect_seen = true,
                        UserEvent::MessageReceived => should_fetch = true,
                    }
                }

                if disconnect_seen {
                    break;
                }

                if should_fetch {
                    if !flush_messages(&mut socket, &repo, user_id, &mut sent_message_ids).await {
                        break;
                    }
                }
            }
        }
    }
}

async fn flush_messages(
    socket: &mut WebSocket,
    repo: &MessageRepository,
    user_id: Uuid,
    sent_ids: &mut HashSet<String>,
) -> bool {
    let mut stream = repo.fetch_pending(user_id);

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                let msg_id_str = msg.id.to_string();
                if sent_ids.contains(&msg_id_str) {
                    continue;
                }

                if let Ok(outgoing) = OutgoingMessage::decode(msg.content.as_slice()) {
                    let envelope = IncomingEnvelope {
                        id: msg_id_str.clone(),
                        r#type: outgoing.r#type,
                        source_user_id: msg.sender_id.to_string(),
                        timestamp: outgoing.timestamp,
                        content: outgoing.content,
                    };

                    let frame = WebSocketFrame { request_id: 0, payload: Some(Payload::Envelope(envelope)) };

                    let mut buf = Vec::new();
                    if frame.encode(&mut buf).is_ok() {
                        if socket.send(WsMessage::Binary(buf.into())).await.is_err() {
                            return false;
                        }
                        sent_ids.insert(msg_id_str);
                    }
                }
            }
            Err(_) => return false,
        }
    }
    true
}
