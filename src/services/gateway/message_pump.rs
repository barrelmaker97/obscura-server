use crate::error::Result;
use crate::proto::obscura::v1 as proto;
use crate::services::gateway::Metrics;
use crate::services::message_service::MessageService;
use axum::extract::ws::Message as WsMessage;
use opentelemetry::KeyValue;
use prost::Message as ProstMessage;
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

/// `MessagePump` coalesces multiple delivery notifications into a single background
/// database poll to avoid overwhelming the database with redundant queries.
pub struct MessagePump {
    notify_tx: mpsc::Sender<()>,
}

impl MessagePump {
    pub fn new(
        device_id: Uuid,
        message_service: MessageService,
        outbound_tx: mpsc::Sender<WsMessage>,
        metrics: Metrics,
        batch_limit: i64,
        max_batch_bytes: usize,
    ) -> Self {
        // Channel size 1 effectively coalesces notifications while a fetch is in progress.
        let (notify_tx, notify_rx) = mpsc::channel(1);

        tokio::spawn(
            async move {
                Self::run_background(device_id, notify_rx, message_service, outbound_tx, metrics, batch_limit, max_batch_bytes).await;
            }
            .instrument(tracing::info_span!("message_pump", "device.id" = %device_id)),
        );

        Self { notify_tx }
    }

    pub fn notify(&self) {
        let _ = self.notify_tx.try_send(());
    }

    async fn run_background(
        device_id: Uuid,
        mut rx: mpsc::Receiver<()>,
        message_service: MessageService,
        outbound_tx: mpsc::Sender<WsMessage>,
        metrics: Metrics,
        limit: i64,
        max_batch_bytes: usize,
    ) {
        let mut cursor: Option<(time::OffsetDateTime, Uuid)> = None;

        while rx.recv().await.is_some() {
            // Continues fetching until the backlog is fully drained for the user.
            while matches!(
                Self::flush_batch(device_id, &message_service, &outbound_tx, &metrics, limit, max_batch_bytes, &mut cursor).await,
                Ok(true)
            ) {}
        }
    }

    #[tracing::instrument(
        err(level = "debug"),
        skip(service, outbound_tx, metrics, cursor),
        fields(user.id = %device_id, batch_count = tracing::field::Empty)
    )]
    async fn flush_batch(
        device_id: Uuid,
        service: &MessageService,
        outbound_tx: &mpsc::Sender<WsMessage>,
        metrics: &Metrics,
        limit: i64,
        max_batch_bytes: usize,
        cursor: &mut Option<(time::OffsetDateTime, Uuid)>,
    ) -> Result<bool> {
        let messages = service.fetch_pending_batch(device_id, *cursor, limit).await?;

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

        let envelopes: Vec<proto::Envelope> = messages
            .into_iter()
            .map(|msg| {
                let timestamp = msg.created_at.map_or_else(
                    || u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(0),
                    |ts| u64::try_from(ts.unix_timestamp_nanos() / 1_000_000).unwrap_or(0),
                );

                proto::Envelope {
                    id: msg.id.as_bytes().to_vec(),
                    sender_id: msg.sender_id.as_bytes().to_vec(),
                    timestamp,
                    message: msg.content,
                }
            })
            .collect();

        // Split envelopes into sub-batches that stay under the WebSocket frame
        // size limit, sending each as a separate EnvelopeBatch frame.
        let mut current_batch: Vec<proto::Envelope> = Vec::new();
        let mut current_size: usize = 0;

        for envelope in envelopes {
            let envelope_size = envelope.encoded_len();

            if !current_batch.is_empty() && current_size + envelope_size > max_batch_bytes {
                Self::send_batch(std::mem::take(&mut current_batch), outbound_tx, metrics).await?;
                current_size = 0;
            }

            current_size += envelope_size;
            current_batch.push(envelope);
        }

        if !current_batch.is_empty() {
            Self::send_batch(current_batch, outbound_tx, metrics).await?;
        }

        Ok(batch_size >= usize::try_from(limit).unwrap_or(usize::MAX))
    }

    async fn send_batch(
        envelopes: Vec<proto::Envelope>,
        outbound_tx: &mpsc::Sender<WsMessage>,
        metrics: &Metrics,
    ) -> Result<bool> {
        let batch = proto::EnvelopeBatch { envelopes };
        let frame = proto::WebSocketFrame {
            payload: Some(proto::web_socket_frame::Payload::EnvelopeBatch(batch)),
        };
        let mut buf = Vec::new();

        if let Err(err) = frame.encode(&mut buf) {
            metrics.outbound_dropped_total.add(1, &[KeyValue::new("reason", "encode_failed")]);
            tracing::warn!(error = ?err, "failed to encode outbound websocket frame");
            return Ok(false);
        }

        if outbound_tx.send(WsMessage::Binary(buf.into())).await.is_err() {
            metrics.outbound_dropped_total.add(1, &[KeyValue::new("reason", "channel_closed")]);
            return Ok(false);
        }

        Ok(true)
    }
}
