use crate::services::gateway::Metrics;
use crate::services::message_service::MessageService;
use crate::proto::obscura::v1::{EncryptedMessage, Envelope, WebSocketFrame, web_socket_frame::Payload};
use axum::extract::ws::Message as WsMessage;
use opentelemetry::KeyValue;
use prost::Message as ProstMessage;
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

/// MessagePump coalesces multiple delivery notifications into a single background
/// database poll to avoid overwhelming the database with redundant queries.
pub struct MessagePump {
    notify_tx: mpsc::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl MessagePump {
    pub fn new(
        user_id: Uuid,
        message_service: MessageService,
        outbound_tx: mpsc::Sender<WsMessage>,
        metrics: Metrics,
        batch_limit: i64,
    ) -> Self {
        // Channel size 1 effectively coalesces notifications while a fetch is in progress.
        let (notify_tx, notify_rx) = mpsc::channel(1);

        let task = tokio::spawn(
            async move {
                Self::run_background(
                    user_id,
                    notify_rx,
                    message_service,
                    outbound_tx,
                    metrics,
                    batch_limit,
                )
                .await;
            }
            .instrument(tracing::info_span!("message_pump", user.id = %user_id)),
        );

        Self { notify_tx, task }
    }

    pub fn notify(&self) {
        let _ = self.notify_tx.try_send(());
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    async fn run_background(
        user_id: Uuid,
        mut rx: mpsc::Receiver<()>,
        message_service: MessageService,
        outbound_tx: mpsc::Sender<WsMessage>,
        metrics: Metrics,
        limit: i64,
    ) {
        let mut cursor: Option<(time::OffsetDateTime, Uuid)> = None;

        while rx.recv().await.is_some() {
            // Continues fetching until the backlog is fully drained for the user.
            while let Ok(true) =
                Self::flush_batch(user_id, &message_service, &outbound_tx, &metrics, limit, &mut cursor)
                    .await
            {}
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
        metrics: &Metrics,
        limit: i64,
        cursor: &mut Option<(time::OffsetDateTime, Uuid)>,
    ) -> crate::error::Result<bool> {
        let messages = service.fetch_pending_batch(user_id, *cursor, limit).await?;

        if messages.is_empty() {
            return Ok(false);
        }

        let batch_size = messages.len();
        tracing::Span::current().record("batch.count", batch_size);

        if let Some(last_msg) = messages.last()
            && let Some(ts) = last_msg.created_at
        {
            *cursor = Some((ts, last_msg.id));
        }

        for msg in messages {
            let timestamp = msg
                .created_at
                .map(|ts| (ts.unix_timestamp_nanos() / 1_000_000) as u64)
                .unwrap_or_else(|| {
                    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64
                });

            let envelope = Envelope {
                id: msg.id.to_string(),
                source_user_id: msg.sender_id.to_string(),
                timestamp,
                message: Some(EncryptedMessage { r#type: msg.message_type, content: msg.content }),
            };

            let frame = WebSocketFrame { payload: Some(Payload::Envelope(envelope)) };
            let mut buf = Vec::new();

            if frame.encode(&mut buf).is_ok()
                && outbound_tx.send(WsMessage::Binary(buf.into())).await.is_err()
            {
                metrics
                    .outbound_dropped_total
                    .add(1, &[KeyValue::new("reason", "buffer_full")]);
                return Ok(false);
            }
        }

        Ok(batch_size >= limit as usize)
    }
}
