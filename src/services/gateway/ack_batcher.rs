use crate::services::gateway::Metrics;
use crate::services::message_service::MessageService;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::Instrument;
use uuid::Uuid;

/// `AckBatcher` decouples fast WebSocket ACKs from slow database deletes and
/// reduces database overhead by batching multiple deletions into a single query.
pub struct AckBatcher {
    tx: mpsc::Sender<Uuid>,
    metrics: Metrics,
    task: tokio::task::JoinHandle<()>,
}

impl AckBatcher {
    pub fn new(
        user_id: Uuid,
        message_service: MessageService,
        metrics: Metrics,
        buffer_size: usize,
        batch_size: usize,
        flush_interval_ms: u64,
    ) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);

        let batcher_metrics = metrics.clone();
        let task = tokio::spawn(
            async move {
                Self::run_background(rx, message_service, batcher_metrics, batch_size, flush_interval_ms).await;
            }
            .instrument(tracing::info_span!("ack_batcher", user_id = %user_id)),
        );

        Self { tx, metrics, task }
    }

    pub fn push(&self, msg_ids: Vec<Uuid>) {
        for msg_id in msg_ids {
            if self.tx.try_send(msg_id).is_err() {
                tracing::warn!(message_id = %msg_id, "Dropped ACK due to full buffer");
                self.metrics.ack_queue_dropped_total.add(1, &[]);
            }
        }
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    async fn run_background(
        mut rx: mpsc::Receiver<Uuid>,
        message_service: MessageService,
        metrics: Metrics,
        batch_size: usize,
        flush_interval_ms: u64,
    ) {
        loop {
            let mut batch = Vec::new();
            let timeout = tokio::time::sleep(Duration::from_millis(flush_interval_ms));
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
                    () = &mut timeout => break,
                }
            }

            if !batch.is_empty() {
                tracing::debug!(batch_size = batch.len(), "Flushing ACK batch");
                metrics.ack_batch_size.record(batch.len() as u64, &[]);
                if let Err(e) = message_service.delete_batch(&batch).await {
                    tracing::error!(error = %e, "Failed to delete message batch");
                }
            }
        }
    }
}
