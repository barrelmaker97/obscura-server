use crate::config::WsConfig;
use crate::proto::obscura::v1::{WebSocketFrame, web_socket_frame::Payload};
use crate::services::gateway::{Metrics, ack_batcher::AckBatcher, message_pump::MessagePump};
use crate::services::message_service::MessageService;
use crate::services::notification_service::{NotificationService, UserEvent};
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures::{SinkExt, StreamExt};
use prost::Message;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

pub struct Session {
    pub user_id: Uuid,
    pub request_id: String,
    pub socket: WebSocket,
    pub message_service: MessageService,
    pub notifier: Arc<dyn NotificationService>,
    pub metrics: Metrics,
    pub config: WsConfig,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

impl Session {
    #[tracing::instrument(
        name = "websocket_session",
        skip(self),
        fields(
            user_id = %self.user_id,
            request_id = %self.request_id,
            otel.kind = "server",
            ws.session_id = %Uuid::new_v4()
        )
    )]
    pub(crate) async fn run(self) {
        // Destructuring allows independent mutable access to fields while the socket
        // is split into sink and stream halves.
        let Self { user_id, socket, message_service, notifier, metrics, config, mut shutdown_rx, .. } = self;

        metrics.active_connections.add(1, &[]);
        tracing::info!("WebSocket connected");

        let mut notification_rx = notifier.subscribe(user_id).await;
        let (mut ws_sink, mut ws_stream) = socket.split();

        // Components are initialized here inside the 'websocket_session' span
        // to ensure they are recorded as child spans in traces.
        let (outbound_tx, mut outbound_rx) = mpsc::channel(config.outbound_buffer_size);

        let ack_batcher = AckBatcher::new(
            user_id,
            message_service.clone(),
            metrics.clone(),
            config.ack_buffer_size,
            config.ack_batch_size,
            config.ack_flush_interval_ms,
        );

        let message_pump = MessagePump::new(
            user_id,
            message_service.clone(),
            outbound_tx,
            metrics.clone(),
            message_service.batch_limit(),
        );

        message_pump.notify();

        loop {
            // Priority is given to shutdown and high-frequency events to ensure
            // the server remains responsive to control signals.
            if *shutdown_rx.borrow() {
                tracing::info!("Shutdown signal received, closing WebSocket");
                let _ = ws_sink
                    .send(WsMessage::Close(Some(axum::extract::ws::CloseFrame {
                        code: axum::extract::ws::close_code::AWAY,
                        reason: "Server shutting down".into(),
                    })))
                    .await;
                break;
            }

            tokio::select! {
                biased;

                _ = shutdown_rx.changed() => {}

                msg = ws_stream.next() => {
                    let continue_loop = match msg {
                        Some(Ok(WsMessage::Binary(bin))) => {
                            if let Ok(frame) = WebSocketFrame::decode(bin.as_ref()) {
                                if let Some(Payload::Ack(ack)) = frame.payload {
                                    if let Ok(msg_id) = Uuid::parse_str(&ack.message_id) {
                                        ack_batcher.push(msg_id);
                                    } else {
                                        tracing::warn!("Received ACK with invalid UUID");
                                    }
                                } else {
                                    tracing::warn!("Received unexpected Protobuf payload type");
                                }
                            } else {
                                tracing::warn!("Failed to decode WebSocket frame");
                            }
                            true
                        }
                        Some(Ok(WsMessage::Close(_)) | Err(_)) | None => false,
                        Some(Ok(WsMessage::Text(t))) => {
                            tracing::warn!("Received unexpected text message: {}", t);
                            true
                        }
                        Some(Ok(WsMessage::Ping(_))) => {
                            tracing::debug!("Received heartbeat ping from client");
                            true
                        }
                        Some(Ok(WsMessage::Pong(_))) => {
                            tracing::debug!("Received heartbeat pong from client");
                            true
                        }
                    };

                    if !continue_loop { break; }
                }

                msg = outbound_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if ws_sink.send(msg).await.is_err() { break; }
                        }
                        None => break,
                    }
                }

                result = notification_rx.recv() => {
                    let continue_loop = match result {
                        Ok(UserEvent::MessageReceived) | Err(broadcast::error::RecvError::Lagged(_)) => {
                            message_pump.notify();
                            // Drain prevents queue buildup if notifications arrive faster than processing.
                            while notification_rx.try_recv() == Ok(UserEvent::MessageReceived) {
                                 message_pump.notify();
                            }
                            true
                        }
                        Ok(UserEvent::Disconnect) | Err(broadcast::error::RecvError::Closed) => false,
                    };

                     if !continue_loop { break; }
                }
            }
        }

        let _ = ws_sink.close().await;

        // Explicitly abort background tasks to ensure immediate resource cleanup.
        ack_batcher.abort();
        message_pump.abort();

        metrics.active_connections.add(-1, &[]);
        tracing::info!("WebSocket disconnected");
    }
}
