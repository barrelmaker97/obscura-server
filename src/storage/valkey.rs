use crate::config::ValkeyConfig;
use dashmap::DashMap;
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
    pub fn publisher(&self) -> redis::aio::ConnectionManager {
        self.publisher.clone()
    }

    /// Subscribes to a Redis pattern.
    /// If a background listener for this pattern isn't already running, it will be started.
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

        tokio::spawn(
            async move {
                Self::run_pattern_listener(client, pattern_str, tx, shutdown, subscriptions).await;
            }
            .instrument(tracing::info_span!("valkey_pattern_listener", pattern = %pattern)),
        );

        Ok(rx)
    }

    async fn run_pattern_listener(
        client: redis::Client,
        pattern: String,
        tx: broadcast::Sender<ValkeyMessage>,
        mut shutdown: watch::Receiver<bool>,
        subscriptions: Arc<DashMap<String, broadcast::Sender<ValkeyMessage>>>,
    ) {
        let mut backoff = std::time::Duration::from_secs(1);
        let max_backoff = std::time::Duration::from_secs(30);

        loop {
            let mut pubsub = match client.get_async_pubsub().await {
                Ok(ps) => ps,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get async pubsub, retrying in {:?}", backoff);
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {
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
                    _ = tokio::time::sleep(backoff) => {
                        backoff = std::cmp::min(backoff * 2, max_backoff);
                        continue;
                    }
                    _ = shutdown.changed() => break,
                }
            }

            tracing::info!(pattern = %pattern, "Successfully subscribed to pattern");
            backoff = std::time::Duration::from_secs(1);

            let mut message_stream = pubsub.into_on_message();
            use futures::StreamExt;

            loop {
                tokio::select! {
                    _ = shutdown.changed() => return,
                    msg = message_stream.next() => {
                        match msg {
                            Some(msg) => {
                                let valkey_msg = ValkeyMessage {
                                    channel: msg.get_channel_name().to_string(),
                                    payload: msg.get_payload().unwrap_or_default(),
                                };
                                if tx.send(valkey_msg).is_err() {
                                    // If no one is listening, we could potentially stop the listener,
                                    // but for simplicity we'll keep it running until shutdown or
                                    // until we implement a more complex cleanup logic.
                                }
                            }
                            None => {
                                tracing::warn!(pattern = %pattern, "Valkey pubsub connection lost, reconnecting...");
                                break;
                            }
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
    pub async fn publish(&self, channel: &str, payload: u8) -> anyhow::Result<()> {
        let mut conn = self.publisher();
        conn.publish::<_, _, i64>(channel, payload).await?;
        Ok(())
    }
}
