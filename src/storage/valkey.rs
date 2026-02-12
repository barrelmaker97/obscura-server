use crate::config::ValkeyConfig;
use dashmap::DashMap;
use futures::StreamExt;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use tracing::Instrument;

#[derive(Debug, Clone)]
pub struct ValkeyMessage {
    pub channel: String,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct ValkeyClient {
    publisher: redis::aio::ConnectionManager,
    // Maps patterns (e.g. "user:*") to broadcast senders
    subscriptions: Arc<DashMap<String, broadcast::Sender<ValkeyMessage>>>,
    client: redis::Client,
    shutdown: watch::Receiver<bool>,
    channel_capacity: usize,
}

impl ValkeyClient {
    /// Creates a new Valkey client.
    ///
    /// # Errors
    /// Returns an error if the connection fails.
    pub async fn new(
        config: &ValkeyConfig,
        channel_capacity: usize,
        shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<Arc<Self>> {
        let client = redis::Client::open(config.url.as_str())?;
        let publisher = client.get_connection_manager().await?;
        let subscriptions = Arc::new(DashMap::new());

        let valkey_client = Arc::new(Self { publisher, subscriptions, client, shutdown, channel_capacity });

        Ok(valkey_client)
    }

    /// Returns a publisher connection that can be used for standard Redis commands.
    #[must_use]
    pub fn publisher(&self) -> redis::aio::ConnectionManager {
        self.publisher.clone()
    }

    /// Subscribes to a Redis pattern.
    /// If a background listener for this pattern isn't already running, it will be started.
    ///
    /// # Errors
    /// Returns an error if the subscription fails.
    pub async fn subscribe(&self, pattern: &str) -> anyhow::Result<broadcast::Receiver<ValkeyMessage>> {
        if let Some(tx) = self.subscriptions.get(pattern) {
            return Ok(tx.subscribe());
        }

        // Create new broadcast channel for this pattern
        let (tx, rx) = broadcast::channel(self.channel_capacity);
        self.subscriptions.insert(pattern.to_string(), tx.clone());

        // Spawn a background listener for this specific pattern
        let pattern_str = pattern.to_string();
        let client = self.client.clone();
        let shutdown = self.shutdown.clone();
        let subscriptions = Arc::clone(&self.subscriptions);

        // We use a channel to wait for the first successful subscription
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(
            async move {
                Self::run_pattern_listener(client, pattern_str, tx, shutdown, subscriptions, ready_tx).await;
            }
            .instrument(tracing::info_span!("valkey_pattern_listener", pattern = %pattern)),
        );

        // Wait for the listener to be ready (psubscribed)
        let _ = ready_rx.await;

        Ok(rx)
    }

    async fn run_pattern_listener(
        client: redis::Client,
        pattern: String,
        tx: broadcast::Sender<ValkeyMessage>,
        mut shutdown: watch::Receiver<bool>,
        subscriptions: Arc<DashMap<String, broadcast::Sender<ValkeyMessage>>>,
        ready_tx: tokio::sync::oneshot::Sender<()>,
    ) {
        let mut backoff = std::time::Duration::from_secs(1);
        let max_backoff = std::time::Duration::from_secs(30);
        let mut ready_tx = Some(ready_tx);

        loop {
            let mut pubsub = match client.get_async_pubsub().await {
                Ok(ps) => ps,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get async pubsub, retrying in {:?}", backoff);
                    tokio::select! {
                        () = tokio::time::sleep(backoff) => {
                            backoff = std::cmp::min(backoff * 2, max_backoff);
                            continue;
                        }
                        _ = shutdown.changed() => break,
                    }
                }
            };

            if let Err(e) = pubsub.psubscribe(&pattern).await {
                tracing::error!(error = %e, "Failed to psubscribe to {}, retrying in {:?}", pattern, backoff);
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {
                        backoff = std::cmp::min(backoff * 2, max_backoff);
                        continue;
                    }
                    _ = shutdown.changed() => break,
                }
            }

            tracing::info!(pattern = %pattern, "Successfully subscribed to pattern");
            if let Some(rtx) = ready_tx.take() {
                let _ = rtx.send(());
            }
            backoff = std::time::Duration::from_secs(1);

            let mut message_stream = pubsub.into_on_message();

            loop {
                tokio::select! {
                    _ = shutdown.changed() => return,
                    msg = message_stream.next() => {
                        if let Some(msg) = msg {
                            let valkey_msg = ValkeyMessage {
                                channel: msg.get_channel_name().to_string(),
                                payload: msg.get_payload().unwrap_or_default(),
                            };
                            if tx.send(valkey_msg).is_err() {
                                // If no one is listening, we could potentially stop the listener,
                                // but for simplicity we'll keep it running until shutdown or
                                // until we implement a more complex cleanup logic.
                            }
                        } else {
                            tracing::warn!(pattern = %pattern, "Valkey pubsub connection lost, reconnecting...");
                            break;
                        }
                    }
                }
            }

            if *shutdown.borrow() {
                break;
            }
        }

        subscriptions.remove(&pattern);
    }

    /// Helper to publish a message directly.
    ///
    /// # Errors
    /// Returns an error if the publish fails.
    pub async fn publish(&self, channel: &str, payload: &[u8]) -> anyhow::Result<()> {
        let mut conn = self.publisher();
        conn.publish::<_, _, i64>(channel, payload).await?;
        Ok(())
    }
}
