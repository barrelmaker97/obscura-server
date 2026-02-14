use crate::config::PubSubConfig;
use backon::{ExponentialBuilder, Retryable};
use dashmap::DashMap;
use futures::StreamExt;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use tracing::Instrument;

#[derive(Debug, Clone)]
pub struct PubSubMessage {
    pub channel: String,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct RedisClient {
    publisher: redis::aio::ConnectionManager,
    // Maps patterns (e.g. "user:*") to broadcast senders
    subscriptions: Arc<DashMap<String, broadcast::Sender<PubSubMessage>>>,
    client: redis::Client,
    shutdown: watch::Receiver<bool>,
    channel_capacity: usize,
    config: PubSubConfig,
}

impl RedisClient {
    /// Creates a new Redis-based `PubSub` client.
    ///
    /// # Errors
    /// Returns an error if the connection fails.
    pub async fn new(
        config: &PubSubConfig,
        channel_capacity: usize,
        shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<Arc<Self>> {
        let client = redis::Client::open(config.url.as_str())?;
        let publisher = client.get_connection_manager().await?;
        let subscriptions = Arc::new(DashMap::new());

        let redis_client =
            Arc::new(Self { publisher, subscriptions, client, shutdown, channel_capacity, config: config.clone() });

        Ok(redis_client)
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
    pub async fn subscribe(&self, pattern: &str) -> anyhow::Result<broadcast::Receiver<PubSubMessage>> {
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
        let config = self.config.clone();

        // We use a channel to wait for the first successful subscription
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(
            async move {
                Self::run_pattern_listener(client, pattern_str, tx, shutdown, subscriptions, config, ready_tx).await;
            }
            .instrument(tracing::info_span!("pubsub_listener", pattern = %pattern)),
        );

        // Wait for the listener to be ready (psubscribed)
        let _ = ready_rx.await;

        Ok(rx)
    }

    async fn run_pattern_listener(
        client: redis::Client,
        pattern: String,
        tx: broadcast::Sender<PubSubMessage>,
        mut shutdown: watch::Receiver<bool>,
        subscriptions: Arc<DashMap<String, broadcast::Sender<PubSubMessage>>>,
        config: PubSubConfig,
        ready_tx: tokio::sync::oneshot::Sender<()>,
    ) {
        let retry_strategy = ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_secs(config.min_backoff_secs))
            .with_max_delay(std::time::Duration::from_secs(config.max_backoff_secs));

        let mut ready_tx = Some(ready_tx);

        loop {
            let pubsub_result = (|| async {
                let mut pubsub = client.get_async_pubsub().await?;
                pubsub.psubscribe(&pattern).await?;
                Ok::<redis::aio::PubSub, redis::RedisError>(pubsub)
            })
            .retry(&retry_strategy)
            .when(|e| {
                tracing::warn!(error = %e, "Failed to subscribe to pubsub, retrying...");
                true
            })
            .notify(|e, duration| {
                tracing::debug!("Pubsub subscription retry in {:?} due to error: {:?}", duration, e);
            })
            .await;

            let pubsub: redis::aio::PubSub = match pubsub_result {
                Ok(ps) => ps,
                Err(e) => {
                    tracing::error!(error = %e, "Pubsub subscription failed after retries");
                    break;
                }
            };

            tracing::info!(pattern = %pattern, "Successfully subscribed to pubsub");
            if let Some(rtx) = ready_tx.take() {
                let _ = rtx.send(());
            }

            let mut message_stream = pubsub.into_on_message();

            loop {
                tokio::select! {
                    _ = shutdown.changed() => return,
                    msg = message_stream.next() => {
                        if let Some(msg) = msg {
                            let pubsub_msg = PubSubMessage {
                                channel: msg.get_channel_name().to_string(),
                                payload: msg.get_payload().unwrap_or_default(),
                            };
                            if tx.send(pubsub_msg).is_err() {
                                // If no one is listening, we could potentially stop the listener,
                                // but for simplicity we'll keep it running until shutdown.
                            }
                        } else {
                            tracing::warn!(pattern = %pattern, "Pubsub connection lost, reconnecting...");
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

    /// Adds a member to a sorted set with a score.
    ///
    /// # Errors
    /// Returns an error if the ZADD fails.
    pub async fn zadd(&self, key: &str, member: &str, score: f64) -> anyhow::Result<()> {
        let mut conn = self.publisher();
        let _: i64 = conn.zadd(key, score, member).await?;
        Ok(())
    }

    /// Removes a member from a sorted set.
    ///
    /// # Errors
    /// Returns an error if the ZREM fails.
    pub async fn zrem(&self, key: &str, member: &str) -> anyhow::Result<i64> {
        let mut conn = self.publisher();
        let removed: i64 = conn.zrem(key, member).await?;
        Ok(removed)
    }

    /// Fetches items from a sorted set that are due (score <= `max_score`).
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub async fn zrange_byscore_limit(&self, key: &str, max_score: f64, limit: isize) -> anyhow::Result<Vec<String>> {
        let mut conn = self.publisher();
        let members: Vec<String> = redis::cmd("ZRANGEBYSCORE")
            .arg(key)
            .arg("-inf")
            .arg(max_score)
            .arg("LIMIT")
            .arg(0)
            .arg(limit)
            .query_async(&mut conn)
            .await?;
        Ok(members)
    }

    /// Pings the Redis server to check connectivity.
    ///
    /// # Errors
    /// Returns an error if the ping fails.
    pub async fn ping(&self) -> anyhow::Result<()> {
        let mut conn = self.publisher();
        redis::cmd("PING").query_async::<String>(&mut conn).await?;
        Ok(())
    }
}
