use crate::config::WsConfig;
use crate::domain::notification::UserEvent;
use crate::proto::obscura::v1 as proto;
use crate::services::gateway::{Metrics, ack_batcher::AckBatcher, message_pump::MessagePump};
use crate::services::key_service::KeyService;
use crate::services::message_service::MessageService;
use crate::services::notification_service::NotificationService;
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures::{SinkExt, StreamExt};
use prost::Message;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

pub struct Session {
    pub user_id: Uuid,
    pub request_id: String,
    pub socket: WebSocket,
    pub message_service: MessageService,
    pub key_service: KeyService,
    pub notifier: NotificationService,
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
    #[allow(clippy::too_many_lines)]
    pub(crate) async fn run(self) {
        // Destructuring allows independent mutable access to fields while the socket
        // is split into sink and stream halves.
        let Self { user_id, socket, message_service, key_service, notifier, metrics, config, mut shutdown_rx, .. } =
            self;

        metrics.active_connections.add(1, &[]);
        tracing::info!("WebSocket connected");

        // Immediately cancel any pending push notifications since the user is now connected.
        notifier.cancel_pending_notifications(user_id).await;

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
            config.message_fetch_batch_size,
        );

        message_pump.notify();

        let mut last_seen = tokio::time::Instant::now();
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(config.ping_interval_secs.max(1)));
        // First tick happens immediately, we skip it to start probing after the first interval.
        ping_interval.tick().await;
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Timer for coalescing PreKeyLow notifications (Debounce window)
        let mut prekey_low_timer = std::pin::pin!(tokio::time::sleep(std::time::Duration::from_secs(3600 * 24 * 365)));
        let mut prekey_low_pending = false;

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

                _ = ping_interval.tick() => {
                    let now = tokio::time::Instant::now();
                    let timeout = std::time::Duration::from_secs(config.ping_interval_secs + config.ping_timeout_secs);

                    if now.duration_since(last_seen) > timeout {
                        tracing::warn!(
                            last_seen_secs = %now.duration_since(last_seen).as_secs(),
                            "WebSocket connection timed out (no pong/activity), closing"
                        );
                        break;
                    }

                    if ws_sink.send(WsMessage::Ping(Vec::new().into())).await.is_err() {
                        break;
                    }
                }

                msg = ws_stream.next() => {
                    let continue_loop = match msg {
                        Some(Ok(msg)) => {
                            last_seen = tokio::time::Instant::now();
                            match msg {
                                WsMessage::Binary(bin) => {
                                    if let Ok(frame) = proto::WebSocketFrame::decode(bin.as_ref()) {
                                        if let Some(proto::web_socket_frame::Payload::Ack(ack)) = frame.payload {
                                            let mut uuids = Vec::new();

                                            if !ack.message_ids.is_empty() {
                                                metrics.acks_received_total.add(1, &[]);
                                            }
                                            for id_bytes in ack.message_ids {
                                                if let Ok(id) = Uuid::from_slice(&id_bytes) {
                                                    uuids.push(id);
                                                } else {
                                                    tracing::warn!(
                                                        len = id_bytes.len(),
                                                        hex = %hex::encode(&id_bytes),
                                                        "Received ACK with invalid UUID bytes in list (expected 16)"
                                                    );
                                                }
                                            }

                                            if !uuids.is_empty() {
                                                // Immediately cancel push notifications to avoid "phantom buzzes"
                                                // Run as fire-and-forget task to avoid blocking the WebSocket loop
                                                let notifier_clone = notifier.clone();
                                                tokio::spawn(async move {
                                                    notifier_clone.cancel_pending_notifications(user_id).await;
                                                });
                                                ack_batcher.push(uuids);
                                            }
                                        } else {
                                            tracing::warn!("Received unexpected Protobuf payload type");
                                        }
                                    } else {
                                        tracing::warn!("Failed to decode WebSocket frame");
                                    }
                                    true
                                }
                                WsMessage::Text(t) => {
                                    tracing::warn!("Received unexpected text message: {}", t);
                                    true
                                }
                                WsMessage::Ping(_) => {
                                    tracing::debug!("Received heartbeat ping from client");
                                    // axum automatically responds with Pong to protocol-level Pings
                                    true
                                }
                                WsMessage::Pong(_) => {
                                    tracing::debug!("Received heartbeat pong from client");
                                    true
                                }
                                WsMessage::Close(_) => false,
                            }
                        }
                        Some(Err(_)) | None => false,
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
                            true
                        }
                        Ok(UserEvent::PreKeyLow) => {
                            // Start or reset the debounce timer (500ms window)
                            prekey_low_pending = true;
                            prekey_low_timer.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_millis(500));
                            true
                        }
                        Ok(UserEvent::Disconnect) | Err(broadcast::error::RecvError::Closed) => false,
                    };

                     if !continue_loop { break; }
                }

                // Handle coalesced PreKeyLow frame delivery (Low priority)
                () = &mut prekey_low_timer, if prekey_low_pending => {
                    match key_service.check_pre_key_status(user_id).await {
                        Ok(Some(status)) => {
                            prekey_low_pending = false;
                            prekey_low_timer.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_secs(3600 * 24 * 365));

                            let frame = proto::WebSocketFrame {
                                payload: Some(proto::web_socket_frame::Payload::PreKeyStatus(proto::PreKeyStatus {
                                    one_time_pre_key_count: status.one_time_pre_key_count,
                                    min_threshold: status.min_threshold,
                                })),
                            };
                            let mut buf = Vec::new();
                            if frame.encode(&mut buf).is_ok()
                                && ws_sink.send(WsMessage::Binary(buf.into())).await.is_err() {
                                    break;
                                }
                        }
                        Ok(None) => {
                            // User is no longer low (refilled?)
                            prekey_low_pending = false;
                            prekey_low_timer.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_secs(3600 * 24 * 365));
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to check pre-key status for coalesced frame");
                        }
                    }
                }
            }
        }

        let _ = ws_sink.close().await;

        // Explicitly abort background tasks to ensure immediate resource cleanup.
        message_pump.abort();

        metrics.active_connections.add(-1, &[]);
        tracing::info!("WebSocket disconnected");
    }
}
