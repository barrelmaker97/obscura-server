use crate::config::WsConfig;
use crate::core::key_service::KeyService;
use crate::core::message_service::MessageService;
use crate::core::notification::{Notifier, UserEvent};
use crate::proto::obscura::v1::{EncryptedMessage, Envelope, WebSocketFrame, web_socket_frame::Payload};
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures::{SinkExt, StreamExt};
use opentelemetry::{KeyValue, global, metrics::{Counter, Histogram, UpDownCounter}};
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::{Instrument, error, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct GatewayMetrics {
    pub websocket_ack_batch_size: Histogram<u64>,
    pub websocket_outbound_dropped_total: Counter<u64>,
    pub websocket_active_connections: UpDownCounter<i64>,
    pub websocket_ack_queue_dropped_total: Counter<u64>,
}

impl GatewayMetrics {
    pub fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            websocket_ack_batch_size: meter
                .u64_histogram("websocket_ack_batch_size")
                .with_description("Size of ACK batches processed")
                .build(),
            websocket_outbound_dropped_total: meter
                .u64_counter("websocket_outbound_dropped_total")
                .with_description("Total messages dropped due to full outbound buffer")
                .build(),
            websocket_active_connections: meter
                .i64_up_down_counter("websocket_active_connections")
                .with_description("Number of active WebSocket connections")
                .build(),
            websocket_ack_queue_dropped_total: meter
                .u64_counter("websocket_ack_queue_dropped_total")
                .with_description("Total ACKs dropped due to full buffer")
                .build(),
        }
    }
}

impl Default for GatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct GatewayService {
    message_service: MessageService,
    key_service: KeyService,
    notifier: Arc<dyn Notifier>,
    config: WsConfig,
    metrics: GatewayMetrics,
}

impl GatewayService {
    pub fn new(
        message_service: MessageService,
        key_service: KeyService,
        notifier: Arc<dyn Notifier>,
        config: WsConfig,
    ) -> Self {
        Self {
            message_service,
            key_service,
            notifier,
            config,
            metrics: GatewayMetrics::new(),
        }
    }

    pub async fn handle_socket(&self, mut socket: WebSocket, user_id: Uuid, request_id: String, shutdown_rx: tokio::sync::watch::Receiver<bool>) {
        let span = tracing::info_span!(
            "websocket_session",
            "request_id" = %request_id,
            "user.id" = %user_id,
            "otel.kind" = "server",
            "ws.session_id" = %Uuid::new_v4()
        );

        let service = self.clone(); // Clone for the async block

        async move {
            // Increment connection gauge
            service.metrics.websocket_active_connections.add(1, &[]);

            tracing::info!("WebSocket connected");
            let mut rx = service.notifier.subscribe(user_id);

            match service.key_service.fetch_identity_key(user_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    warn!("User connected but has no identity key");
                    let _ = socket.close().await;
                    service.metrics.websocket_active_connections.add(-1, &[]);
                    return;
                }
                Err(e) => {
                    error!(error = %e, "Failed to fetch identity key");
                    let _ = socket.close().await;
                    service.metrics.websocket_active_connections.add(-1, &[]);
                    return;
                }
            }

            let (mut ws_sink, mut ws_stream) = socket.split();

            match service.key_service.check_pre_key_status(user_id).await {
                Ok(Some(status)) => {
                    let frame = WebSocketFrame { payload: Some(Payload::PreKeyStatus(status)) };
                    let mut buf = Vec::new();
                    if frame.encode(&mut buf).is_ok() {
                        let _ = ws_sink.send(WsMessage::Binary(buf.into())).await;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to check pre-key status");
                }
                _ => {}
            }

            let (outbound_tx, mut outbound_rx) = mpsc::channel(service.config.outbound_buffer_size);
            let (fetch_trigger, fetch_signal) = mpsc::channel(1);
            let (ack_tx, ack_rx) = mpsc::channel(service.config.ack_buffer_size);

            let session = Arc::new(GatewaySession::new(user_id, service.message_service.clone(), outbound_tx, service.metrics.clone()));

            let (mut db_poller_task, mut ack_processor_task) = session.clone().spawn_background_tasks(
                fetch_signal,
                ack_rx,
                service.config.ack_batch_size,
                service.config.ack_flush_interval_ms,
            );

            let mut shutdown_rx = shutdown_rx.clone();

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
                        match msg {
                            Some(Ok(WsMessage::Binary(bin))) => {
                                 match WebSocketFrame::decode(bin.as_ref()) {
                                     Ok(frame) => {
                                         if let Some(Payload::Ack(ack)) = frame.payload {
                                             match Uuid::parse_str(&ack.message_id) {
                                                 Ok(msg_id) => {
                                                     if ack_tx.try_send(msg_id).is_err() {
                                                         warn!(message_id = %msg_id, "Dropped ACK due to full buffer");
                                                         service.metrics.websocket_ack_queue_dropped_total.add(1, &[]);
                                                     }
                                                 }
                                                 Err(_) => {
                                                     warn!("Received ACK with invalid UUID");
                                                 }
                                             }
                                         }
                                     }
                                     Err(e) => {
                                         warn!(error = %e, "Failed to decode WebSocket frame");
                                     }
                                 }
                            }
                            Some(Ok(WsMessage::Close(_))) => break,
                            Some(Err(e)) => {
                                warn!(error = %e, "WebSocket error");
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }


                    res = outbound_rx.recv() => {
                        match res {
                            Some(msg) => {
                                if ws_sink.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }

                    result = rx.recv() => {
                        match result {
                            Ok(UserEvent::MessageReceived) => {
                                let _ = fetch_trigger.try_send(());
                            }
                            Ok(UserEvent::Disconnect) => break,
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                let _ = fetch_trigger.try_send(());
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }

                        // Drain any pending events
                        let mut disconnect_seen = false;
                        while let Ok(evt) = rx.try_recv() {
                            match evt {
                                UserEvent::Disconnect => disconnect_seen = true,
                                UserEvent::MessageReceived => { let _ = fetch_trigger.try_send(()); }
                            }
                        }
                        if disconnect_seen { break; }
                    }

                    _ = &mut db_poller_task => break,
                    _ = &mut ack_processor_task => break,
                }
            }

            let _ = ws_sink.close().await;
            db_poller_task.abort();
            ack_processor_task.abort();
            service.metrics.websocket_active_connections.add(-1, &[]);
            tracing::info!("WebSocket disconnected");
        }
        .instrument(span)
        .await;
    }
}

struct GatewaySession {
    user_id: Uuid,
    message_service: MessageService,
    outbound_tx: mpsc::Sender<WsMessage>,
    metrics: GatewayMetrics,
}

impl GatewaySession {
    fn new(user_id: Uuid, message_service: MessageService, outbound_tx: mpsc::Sender<WsMessage>, metrics: GatewayMetrics) -> Self {
        Self {
            user_id,
            message_service,
            outbound_tx,
            metrics,
        }
    }

    fn spawn_background_tasks(
        self: Arc<Self>,
        fetch_signal: mpsc::Receiver<()>,
        ack_rx: mpsc::Receiver<Uuid>,
        ack_batch_size: usize,
        ack_flush_interval_ms: u64,
    ) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
        let poller_session = self.clone();
        let db_poller_task = tokio::spawn(async move {
            poller_session.run_poller(fetch_signal).await;
        });

        let ack_session = self.clone();
        let ack_processor_task = tokio::spawn(async move {
            ack_session.run_ack_processor(ack_rx, ack_batch_size, ack_flush_interval_ms).await;
        });

        (db_poller_task, ack_processor_task)
    }

    async fn run_poller(&self, mut fetch_signal: mpsc::Receiver<()>) {
        let mut cursor: Option<(time::OffsetDateTime, Uuid)> = None;
        let batch_limit = self.message_service.batch_limit();

        // Initial fetch
        match self.flush_messages(batch_limit, &mut cursor).await {
            Ok(false) | Err(_) => return,
            Ok(true) => {}
        }

        while fetch_signal.recv().await.is_some() {
            match self.flush_messages(batch_limit, &mut cursor).await {
                Ok(true) => {}
                Ok(false) | Err(_) => break,
            }
        }
    }

    async fn run_ack_processor(&self, mut ack_rx: mpsc::Receiver<Uuid>, batch_size: usize, flush_interval_ms: u64) {
        loop {
            let mut batch = Vec::new();
            let timeout = tokio::time::sleep(std::time::Duration::from_millis(flush_interval_ms));
            tokio::pin!(timeout);

            loop {
                tokio::select! {
                    res = ack_rx.recv() => {
                        match res {
                            Some(id) => {
                                batch.push(id);
                                if batch.len() >= batch_size {
                                    break;
                                }
                            }
                            None => return,
                        }
                    }
                    _ = &mut timeout => {
                        break;
                    }
                }
            }

            if !batch.is_empty() {
                self.metrics.websocket_ack_batch_size.record(batch.len() as u64, &[]);

                // delete_batch is instrumented, so it will log errors internally.
                let _ = self.message_service.delete_batch(&batch).await;
            }
        }
    }

    #[tracing::instrument(
        err(level = "debug"),
        skip(self, cursor),
        fields(user_id = %self.user_id, batch_count = tracing::field::Empty)
    )]
    async fn flush_messages(
        &self,
        limit: i64,
        cursor: &mut Option<(time::OffsetDateTime, Uuid)>,
    ) -> crate::error::Result<bool> {
        loop {
            match self.message_service.fetch_pending_batch(self.user_id, *cursor, limit).await {
                Ok(messages) => {
                    if messages.is_empty() {
                        break;
                    }

                    let batch_size = messages.len();
                    tracing::Span::current().record("batch.count", batch_size);

                    if let Some(last_msg) = messages.last()
                        && let Some(ts) = last_msg.created_at
                    {
                        *cursor = Some((ts, last_msg.id));
                    }

                    for msg in messages {
                        let timestamp =
                            msg.created_at.map(|ts| (ts.unix_timestamp_nanos() / 1_000_000) as u64).unwrap_or_else(
                                || (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64,
                            );

                        let envelope = Envelope {
                            id: msg.id.to_string(),
                            source_user_id: msg.sender_id.to_string(),
                            timestamp,
                            message: Some(EncryptedMessage { r#type: msg.message_type, content: msg.content }),
                        };

                        let frame = WebSocketFrame { payload: Some(Payload::Envelope(envelope)) };

                        let mut buf = Vec::new();
                        if frame.encode(&mut buf).is_ok()
                            && self.outbound_tx.send(WsMessage::Binary(buf.into())).await.is_err()
                        {
                            self.metrics
                                .websocket_outbound_dropped_total
                                .add(1, &[KeyValue::new("reason", "buffer_full")]);

                            return Ok(false);
                        }
                    }

                    if batch_size < limit as usize {
                        break;
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok(true)
    }
}
