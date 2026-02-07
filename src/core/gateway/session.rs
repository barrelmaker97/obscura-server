use crate::config::WsConfig;
use crate::core::message_service::MessageService;
use crate::core::notification::{Notifier, UserEvent};
use crate::proto::obscura::v1::{EncryptedMessage, Envelope, WebSocketFrame, web_socket_frame::Payload};
use crate::core::gateway::GatewayMetrics;
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures::{SinkExt, StreamExt};
use opentelemetry::KeyValue;
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::warn;
use uuid::Uuid;

pub struct Session {
    user_id: Uuid,
    request_id: String,
    socket: WebSocket,
    notifier: Arc<dyn Notifier>,
    metrics: GatewayMetrics,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,

    // Components
    ack_batcher: AckBatcher,
    message_pump: MessagePump,
    outbound_rx: mpsc::Receiver<WsMessage>,
}

pub struct SessionParams {
    pub user_id: Uuid,
    pub request_id: String,
    pub socket: WebSocket,
    pub message_service: MessageService,
    pub notifier: Arc<dyn Notifier>,
    pub metrics: GatewayMetrics,
    pub config: WsConfig,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

impl Session {
    pub fn new(params: SessionParams) -> Self {
        let (outbound_tx, outbound_rx) = mpsc::channel(params.config.outbound_buffer_size);

        let ack_batcher = AckBatcher::new(
            params.message_service.clone(),
            params.metrics.clone(),
            params.config.ack_buffer_size,
            params.config.ack_batch_size,
            params.config.ack_flush_interval_ms,
        );

        let message_pump = MessagePump::new(
            params.user_id,
            params.message_service.clone(),
            outbound_tx,
            params.metrics.clone(),
            params.message_service.batch_limit(),
        );

        Self {
            user_id: params.user_id,
            request_id: params.request_id,
            socket: params.socket,
            notifier: params.notifier,
            metrics: params.metrics,
            shutdown_rx: params.shutdown_rx,
            ack_batcher,
            message_pump,
            outbound_rx,
        }
    }

    #[tracing::instrument(
        name = "websocket_session",
        skip(self),
        fields(
            user.id = %self.user_id,
            request_id = %self.request_id,
            otel.kind = "server",
            ws.session_id = %Uuid::new_v4()
        )
    )]
    pub async fn run(self) {
        let Session {
            user_id,
            socket,
            notifier,
            metrics,
            mut shutdown_rx,
            ack_batcher,
            message_pump,
            mut outbound_rx,
            ..
        } = self;

        metrics.websocket_active_connections.add(1, &[]);
        tracing::info!("WebSocket connected");

        let mut notification_rx = notifier.subscribe(user_id);
        let (mut ws_sink, mut ws_stream) = socket.split();

        // Initial fetch
        message_pump.notify();

        loop {
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
                                        warn!("Received ACK with invalid UUID");
                                    }
                                }
                            } else {
                                warn!("Failed to decode WebSocket frame");
                            }
                            true
                        }
                        Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => false,
                        _ => true,
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
                            while let Ok(UserEvent::MessageReceived) = notification_rx.try_recv() {
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
        metrics.websocket_active_connections.add(-1, &[]);
        tracing::info!("WebSocket disconnected");
    }
}

struct AckBatcher {
    tx: mpsc::Sender<Uuid>,
    metrics: GatewayMetrics,
    _task: tokio::task::JoinHandle<()>,
}

impl AckBatcher {
    fn new(
        message_service: MessageService,
        metrics: GatewayMetrics,
        buffer_size: usize,
        batch_size: usize,
        flush_interval_ms: u64,
    ) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);

        let batcher_metrics = metrics.clone();
        let task = tokio::spawn(async move {
            Self::run_background(rx, message_service, batcher_metrics, batch_size, flush_interval_ms).await;
        });

        Self { tx, metrics, _task: task }
    }

    fn push(&self, msg_id: Uuid) {
        if self.tx.try_send(msg_id).is_err() {
            warn!(message_id = %msg_id, "Dropped ACK due to full buffer");
            self.metrics.websocket_ack_queue_dropped_total.add(1, &[]);
        }
    }

    async fn run_background(
        mut rx: mpsc::Receiver<Uuid>,
        message_service: MessageService,
        metrics: GatewayMetrics,
        batch_size: usize,
        flush_interval_ms: u64,
    ) {
        loop {
            let mut batch = Vec::new();
            let timeout = tokio::time::sleep(std::time::Duration::from_millis(flush_interval_ms));
            tokio::pin!(timeout);

            loop {
                tokio::select! {
                    res = rx.recv() => {
                        match res {
                            Some(id) => {
                                batch.push(id);
                                if batch.len() >= batch_size { break; }
                            }
                            None => return,
                        }
                    }
                    _ = &mut timeout => break,
                }
            }

            if !batch.is_empty() {
                metrics.websocket_ack_batch_size.record(batch.len() as u64, &[]);
                let _ = message_service.delete_batch(&batch).await;
            }
        }
    }
}

struct MessagePump {
    notify_tx: mpsc::Sender<()>,
    _task: tokio::task::JoinHandle<()>,
}

impl MessagePump {
    fn new(
        user_id: Uuid,
        message_service: MessageService,
        outbound_tx: mpsc::Sender<WsMessage>,
        metrics: GatewayMetrics,
        batch_limit: i64,
    ) -> Self {
        let (notify_tx, notify_rx) = mpsc::channel(1);

        let task = tokio::spawn(async move {
            Self::run_background(user_id, notify_rx, message_service, outbound_tx, metrics, batch_limit).await;
        });

        Self { notify_tx, _task: task }
    }

    fn notify(&self) {
        let _ = self.notify_tx.try_send(());
    }

    async fn run_background(
        user_id: Uuid,
        mut rx: mpsc::Receiver<()>,
        message_service: MessageService,
        outbound_tx: mpsc::Sender<WsMessage>,
        metrics: GatewayMetrics,
        limit: i64,
    ) {
        let mut cursor: Option<(time::OffsetDateTime, Uuid)> = None;

        while rx.recv().await.is_some() {
            while let Ok(true) = Self::flush_batch(user_id, &message_service, &outbound_tx, &metrics, limit, &mut cursor).await {}
        }
    }

    #[tracing::instrument(
        err(level = "debug"),
        skip(service, outbound_tx, metrics, cursor),
        fields(user_id = %user_id, batch_count = tracing::field::Empty)
    )]
    async fn flush_batch(
        user_id: Uuid,
        service: &MessageService,
        outbound_tx: &mpsc::Sender<WsMessage>,
        metrics: &GatewayMetrics,
        limit: i64,
        cursor: &mut Option<(time::OffsetDateTime, Uuid)>,
    ) -> crate::error::Result<bool> {
        let messages = service.fetch_pending_batch(user_id, *cursor, limit).await?;

        if messages.is_empty() { return Ok(false); }

        let batch_size = messages.len();
        tracing::Span::current().record("batch.count", batch_size);

        if let Some(last_msg) = messages.last()
            && let Some(ts) = last_msg.created_at
        {
            *cursor = Some((ts, last_msg.id));
        }

        for msg in messages {
            let timestamp = msg.created_at
                .map(|ts| (ts.unix_timestamp_nanos() / 1_000_000) as u64)
                .unwrap_or_else(|| (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64);

            let envelope = Envelope {
                id: msg.id.to_string(),
                source_user_id: msg.sender_id.to_string(),
                timestamp,
                message: Some(EncryptedMessage { r#type: msg.message_type, content: msg.content }),
            };

            let frame = WebSocketFrame { payload: Some(Payload::Envelope(envelope)) };
            let mut buf = Vec::new();

            if frame.encode(&mut buf).is_ok()
                 && outbound_tx.send(WsMessage::Binary(buf.into())).await.is_err() {
                    metrics.websocket_outbound_dropped_total.add(1, &[KeyValue::new("reason", "buffer_full")]);
                    return Ok(false);
            }
        }

        Ok(batch_size >= limit as usize)
    }
}
