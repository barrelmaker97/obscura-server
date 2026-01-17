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
use tokio::sync::mpsc;
use prost::Message as ProstMessage;
use serde::Deserialize;
use tracing::{warn, error};
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
    let key_repo = KeyRepository::new(state.pool.clone());

    // Check for Identity Key
    if let Ok(None) = key_repo.fetch_identity_key(user_id).await {
        // No identity key found, close connection
        let _ = socket.close().await;
        return;
    }

    let (mut ws_sink, mut ws_stream) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<WsMessage>(state.config.ws_outbound_buffer_size);
    let (fetch_trigger, mut fetch_signal) = mpsc::channel::<()>(1);
    // Bounded channel for ACKs (DoS protection)
    let (ack_tx, mut ack_rx) = mpsc::channel::<Uuid>(state.config.ws_ack_buffer_size);

    // Writer Task: Message Channel -> Socket
    let mut socket_writer_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // Fetcher Task: Trigger -> DB -> Message Channel
    let pool = state.pool.clone();
    let batch_limit = state.config.message_batch_limit;
    let mut db_poller_task = tokio::spawn(async move {
        let repo = MessageRepository::new(pool);
        let mut cursor: Option<(time::OffsetDateTime, Uuid)> = None;

        // Initial fetch
        if !flush_messages(&outbound_tx, &repo, user_id, batch_limit, &mut cursor).await {
            return;
        }

        while fetch_signal.recv().await.is_some() {
            if !flush_messages(&outbound_tx, &repo, user_id, batch_limit, &mut cursor).await {
                break;
            }
        }
    });

    // ACK Processor Task: Buffer -> DB Batch Delete
    let repo_ack = MessageRepository::new(state.pool.clone());
    let ack_batch_size = state.config.ws_ack_batch_size;
    let ack_flush_interval_ms = state.config.ws_ack_flush_interval_ms;
    
    let mut ack_processor_task = tokio::spawn(async move {
        loop {
            let mut batch = Vec::new();
            let timeout = tokio::time::sleep(std::time::Duration::from_millis(ack_flush_interval_ms));
            tokio::pin!(timeout);

            // Collect batch
            loop {
                tokio::select! {
                    res = ack_rx.recv() => {
                        match res {
                            Some(id) => {
                                batch.push(id);
                                if batch.len() >= ack_batch_size {
                                    break;
                                }
                            }
                            None => return, // Channel closed
                        }
                    }
                    _ = &mut timeout => {
                        break;
                    }
                }
            }

            if !batch.is_empty() {
                if let Err(e) = repo_ack.delete_batch(&batch).await {
                    error!("Failed to process ACK batch: {}", e);
                }
            }
        }
    });

    let mut rx = state.notifier.subscribe(user_id);

    loop {
        tokio::select! {
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(WsMessage::Binary(bin))) => {
                         if let Ok(frame) = WebSocketFrame::decode(bin.as_ref())
                             && let Some(Payload::Ack(ack)) = frame.payload
                             && let Ok(msg_id) = Uuid::parse_str(&ack.message_id) {
                                 // Non-blocking send. If buffer is full, we drop the ACK.
                                 // The server will re-deliver the message later, which is safe.
                                 if let Err(_) = ack_tx.try_send(msg_id) {
                                     warn!("Dropped ACK for message {} due to full buffer", msg_id);
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
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };

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
                    let _ = fetch_trigger.try_send(());
                }
            }
            _ = &mut socket_writer_task => break,
            _ = &mut db_poller_task => break,
            _ = &mut ack_processor_task => break,
        }
    }

    socket_writer_task.abort();
    db_poller_task.abort();
    ack_processor_task.abort();
}

async fn flush_messages(
    tx: &mpsc::Sender<WsMessage>,
    repo: &MessageRepository,
    user_id: Uuid,
    limit: i64,
    cursor: &mut Option<(time::OffsetDateTime, Uuid)>,
) -> bool {
    loop {
        match repo.fetch_pending_batch(user_id, *cursor, limit).await {
            Ok(messages) => {
                if messages.is_empty() {
                    break;
                }

                let batch_size = messages.len();

                // Update cursor for next iteration based on the last message
                if let Some(last_msg) = messages.last()
                    && let Some(ts) = last_msg.created_at {
                        *cursor = Some((ts, last_msg.id));
                }

                for msg in messages {
                    if let Ok(outgoing) = OutgoingMessage::decode(msg.content.as_slice()) {
                        let envelope = IncomingEnvelope {
                            id: msg.id.to_string(),
                            r#type: outgoing.r#type,
                            source_user_id: msg.sender_id.to_string(),
                            timestamp: outgoing.timestamp,
                            content: outgoing.content,
                        };

                        let frame = WebSocketFrame { request_id: 0, payload: Some(Payload::Envelope(envelope)) };

                        let mut buf = Vec::new();
                        if frame.encode(&mut buf).is_ok()
                            && tx.send(WsMessage::Binary(buf.into())).await.is_err() {
                                return false;
                        }
                    }
                }

                if batch_size < limit as usize {
                    break;
                }
            }
            Err(e) => {
                error!("Failed to fetch pending messages for user {}: {}", user_id, e);
                return false;
            }
        }
    }
    true
}
