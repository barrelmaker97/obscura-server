pub(crate) mod ack_batcher;
pub(crate) mod message_pump;
pub(crate) mod session;

use crate::config::WsConfig;
use crate::proto::obscura::v1::{WebSocketFrame, web_socket_frame::Payload};
use crate::services::gateway::session::Session;
use crate::services::key_service::KeyService;
use crate::services::message_service::MessageService;
use crate::services::notification_service::NotificationService;
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures::SinkExt;
use opentelemetry::{
    global,
    metrics::{Counter, Histogram, UpDownCounter},
};
use prost::Message as ProstMessage;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Metrics {
    pub(crate) ack_batch_size: Histogram<u64>,
    pub(crate) outbound_dropped_total: Counter<u64>,
    pub(crate) active_connections: UpDownCounter<i64>,
    pub(crate) ack_queue_dropped_total: Counter<u64>,
}

impl Metrics {
    #[must_use]
    pub(crate) fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            ack_batch_size: meter
                .u64_histogram("websocket_ack_batch_size")
                .with_description("Size of ACK batches processed")
                .build(),
            outbound_dropped_total: meter
                .u64_counter("websocket_outbound_dropped_total")
                .with_description("Total messages dropped due to full outbound buffer")
                .build(),
            active_connections: meter
                .i64_up_down_counter("websocket_active_connections")
                .with_description("Number of active WebSocket connections")
                .build(),
            ack_queue_dropped_total: meter
                .u64_counter("websocket_ack_queue_dropped_total")
                .with_description("Total ACKs dropped due to full buffer")
                .build(),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct GatewayService {
    message_service: MessageService,
    key_service: KeyService,
    notifier: Arc<dyn NotificationService>,
    config: WsConfig,
    metrics: Metrics,
}

impl GatewayService {
    #[must_use]
    pub fn new(
        message_service: MessageService,
        key_service: KeyService,
        notifier: Arc<dyn NotificationService>,
        config: WsConfig,
    ) -> Self {
        Self { message_service, key_service, notifier, config, metrics: Metrics::new() }
    }

    pub async fn handle_socket(
        &self,
        mut socket: WebSocket,
        user_id: Uuid,
        request_id: String,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) {
        // Validation is performed before spawning the session to provide immediate
        // feedback and avoid allocating resources for invalid connections.
        match self.key_service.fetch_identity_key(user_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                tracing::warn!("User connected but has no identity key");
                let _ = socket.close().await;
                return;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch identity key");
                let _ = socket.close().await;
                return;
            }
        }

        // Clients need to know if they are low on pre-keys immediately upon connection
        // to prevent exhausting their bundle during an active session.
        match self.key_service.check_pre_key_status(user_id).await {
            Ok(Some(status)) => {
                let frame = WebSocketFrame { payload: Some(Payload::PreKeyStatus(status)) };
                let mut buf = Vec::new();
                if frame.encode(&mut buf).is_ok() {
                    let _ = socket.send(WsMessage::Binary(buf.into())).await;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to check pre-key status");
            }
            _ => {}
        }

        // 3. Hand over to Session
        let session = Session {
            user_id,
            request_id,
            socket,
            message_service: self.message_service.clone(),
            notifier: self.notifier.clone(),
            metrics: self.metrics.clone(),
            config: self.config.clone(),
            shutdown_rx,
        };

        session.run().await;
    }
}
