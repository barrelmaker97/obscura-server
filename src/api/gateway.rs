use axum::{
    extract::{ws::{Message as WsMessage, WebSocket, WebSocketUpgrade}, State, Query},
    response::IntoResponse,
};
use serde::Deserialize;
use crate::api::AppState;
use crate::api::middleware::verify_jwt;
use crate::storage::message_repo::MessageRepository;
use crate::proto::obscura::v1::{WebSocketFrame, IncomingEnvelope, web_socket_frame::Payload, OutgoingMessage};
use prost::Message as ProstMessage; 
use uuid::Uuid;

#[derive(Deserialize)]
pub struct WsParams {
    token: String,
}

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
    let repo = MessageRepository::new(state.pool);
    let mut rx = state.notifier.subscribe(user_id);

    // Initial check for pending messages on connect
    if let Ok(messages) = repo.fetch_pending(user_id).await {
        for msg in messages {
             if let Ok(outgoing) = OutgoingMessage::decode(msg.content.as_slice()) {
                 let envelope = IncomingEnvelope {
                     id: msg.id.to_string(),
                     r#type: outgoing.r#type,
                     source_user_id: msg.sender_id.to_string(),
                     timestamp: outgoing.timestamp,
                     content: outgoing.content,
                 };
                 
                 let frame = WebSocketFrame {
                     request_id: 0,
                     payload: Some(Payload::Envelope(envelope)),
                 };
                 
                 let mut buf = Vec::new();
                 if frame.encode(&mut buf).is_ok() {
                     if socket.send(WsMessage::Binary(buf.into())).await.is_err() {
                         return;
                     }
                 }
             }
        }
    }

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(WsMessage::Binary(bin))) => {
                         if let Ok(frame) = WebSocketFrame::decode(bin.as_ref()) {
                             if let Some(Payload::Ack(ack)) = frame.payload {
                                 if let Ok(msg_id) = Uuid::parse_str(&ack.message_id) {
                                     let _ = repo.delete(msg_id).await;
                                 }
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
                match result {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Drain any pending notifications to avoid redundant DB checks
                        while rx.try_recv().is_ok() {}

                        if let Ok(messages) = repo.fetch_pending(user_id).await {
                            for msg in messages {
                                 if let Ok(outgoing) = OutgoingMessage::decode(msg.content.as_slice()) {
                                     let envelope = IncomingEnvelope {
                                         id: msg.id.to_string(),
                                         r#type: outgoing.r#type,
                                         source_user_id: msg.sender_id.to_string(),
                                         timestamp: outgoing.timestamp,
                                         content: outgoing.content,
                                     };
                                     
                                     let frame = WebSocketFrame {
                                         request_id: 0,
                                         payload: Some(Payload::Envelope(envelope)),
                                     };
                                     
                                     let mut buf = Vec::new();
                                     if frame.encode(&mut buf).is_ok() {
                                         if socket.send(WsMessage::Binary(buf.into())).await.is_err() {
                                             break;
                                         }
                                     }
                                 }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
}
