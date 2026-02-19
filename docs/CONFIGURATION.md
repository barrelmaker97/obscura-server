# Configuration Reference

Obscura Server is configured via command-line flags or environment variables using the `clap` derive framework. Flags always take precedence over environment variables.

## Global Settings

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--ttl-days` | `OBSCURA_TTL_DAYS` | `30` | Global time-to-live for messages and attachments in days. |

## Server

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--server-host` | `OBSCURA_SERVER_HOST` | `0.0.0.0` | Interface to bind the server to. |
| `--server-port` | `OBSCURA_SERVER_PORT` | `3000` | Primary port for API and WebSockets. |
| `--mgmt-port` | `OBSCURA_SERVER_MGMT_PORT` | `9090` | Management port for health checks and metrics. |
| `--shutdown-timeout-secs` | `OBSCURA_SERVER_SHUTDOWN_TIMEOUT_SECS` | `5` | How long to wait for background tasks to finish during shutdown in seconds. |
| `--trusted-proxies` | `OBSCURA_SERVER_TRUSTED_PROXIES` | `10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.1/32` | Comma-separated list of CIDRs to trust for X-Forwarded-For IP extraction. |

## Database (PostgreSQL)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--db-url` | `OBSCURA_DATABASE_URL` | `postgres://user:password@localhost/signal_server` | PostgreSQL connection string. |
| `--db-max-connections` | `OBSCURA_DATABASE_MAX_CONNECTIONS` | `20` | Maximum number of connections in the pool. |
| `--db-min-connections` | `OBSCURA_DATABASE_MIN_CONNECTIONS` | `5` | Minimum number of connections to keep idle in the pool. |
| `--db-acquire-timeout-secs` | `OBSCURA_DATABASE_ACQUIRE_TIMEOUT_SECS` | `3` | Seconds to wait before timing out on acquiring a connection. |
| `--db-idle-timeout-secs` | `OBSCURA_DATABASE_IDLE_TIMEOUT_SECS` | `600` | Seconds before an idle connection is closed. |
| `--db-max-lifetime-secs` | `OBSCURA_DATABASE_MAX_LIFETIME_SECS` | `1800` | Seconds before a connection is retired and replaced. |

## PubSub (Redis/Valkey)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--pubsub-url` | `OBSCURA_PUBSUB_URL` | `redis://localhost:6379` | Connection URL for the PubSub and job backend. |
| `--pubsub-min-backoff-secs` | `OBSCURA_PUBSUB_MIN_BACKOFF_SECS` | `1` | Minimum backoff time for PubSub reconnection in seconds. |
| `--pubsub-max-backoff-secs` | `OBSCURA_PUBSUB_MAX_BACKOFF_SECS` | `30` | Maximum backoff time for PubSub reconnection in seconds. |

## Authentication

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--jwt-secret` | `OBSCURA_AUTH_JWT_SECRET` | `change_me_in_production` | Secret key for signing JWT access tokens. |
| `--access-token-ttl-secs` | `OBSCURA_AUTH_TOKEN_TTL_SECS` | `900` | Access token time-to-live in seconds. |
| `--refresh-token-ttl-days` | `OBSCURA_AUTH_REFRESH_TOKEN_TTL_DAYS` | `30` | Refresh token time-to-live in days. |

## Rate Limiting

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--rate-limit-per-second` | `OBSCURA_RATE_LIMIT_PER_SECOND` | `10` | Requests per second allowed for standard endpoints. |
| `--rate-limit-burst` | `OBSCURA_RATE_LIMIT_BURST` | `20` | Burst allowance for standard endpoints. |
| `--auth-rate-limit-per-second` | `OBSCURA_RATE_LIMIT_AUTH_PER_SECOND` | `1` | Stricter rate limit for registration and login endpoints. |
| `--auth-rate-limit-burst` | `OBSCURA_RATE_LIMIT_AUTH_BURST` | `3` | Burst allowance for registration and login endpoints. |

## Messaging & Keys

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--max-inbox-size` | `OBSCURA_MESSAGING_INBOX_MAX_SIZE` | `1000` | Maximum number of pending messages per user before pruning. |
| `--messaging-cleanup-interval-secs` | `OBSCURA_MESSAGING_CLEANUP_INTERVAL_SECS` | `300` | How often to run the message cleanup task in seconds. |
| `--batch-limit` | `OBSCURA_MESSAGING_BATCH_LIMIT` | `50` | Maximum number of messages processed in a single DB fetch loop. |
| `--pre-key-refill-threshold` | `OBSCURA_PRE_KEY_REFILL_THRESHOLD` | `20` | Threshold of one-time prekeys to trigger a refill notification. |
| `--max-pre-keys` | `OBSCURA_PRE_KEYS_MAX` | `100` | Maximum number of one-time prekeys allowed per user. |

## Notifications

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--gc-interval-secs` | `OBSCURA_NOTIFICATIONS_GC_INTERVAL_SECS` | `60` | How often to run the notification channel garbage collection. |
| `--global-channel-capacity` | `OBSCURA_NOTIFICATIONS_GLOBAL_CHANNEL_CAPACITY` | `1024` | Capacity of the global notification dispatcher channel. |
| `--user-channel-capacity` | `OBSCURA_NOTIFICATIONS_USER_CHANNEL_CAPACITY` | `64` | Capacity of the per-user notification channel. |
| `--push-delay-secs` | `OBSCURA_NOTIFICATIONS_PUSH_DELAY_SECS` | `2` | Delay in seconds before a push notification is sent as a fallback. |
| `--worker-interval-secs` | `OBSCURA_NOTIFICATIONS_WORKER_INTERVAL_SECS` | `1` | Interval in seconds for the notification worker to poll for jobs. |
| `--worker-concurrency` | `OBSCURA_NOTIFICATIONS_WORKER_CONCURRENCY` | `100` | Maximum concurrent push delivery tasks. |
| `--push-queue-key` | `OBSCURA_NOTIFICATIONS_PUSH_QUEUE_KEY` | `jobs:push_notifications` | Redis key for the push notification job queue. |
| `--channel-prefix` | `OBSCURA_NOTIFICATIONS_CHANNEL_PREFIX` | `user:` | Redis PubSub channel prefix for user notifications. |
| `--visibility-timeout-secs` | `OBSCURA_NOTIFICATIONS_VISIBILITY_TIMEOUT_SECS` | `30` | How long a push job is leased by a worker in seconds. |
| `--janitor-interval-secs` | `OBSCURA_NOTIFICATIONS_JANITOR_INTERVAL_SECS` | `5` | How often the invalid token janitor flushes to the database. |
| `--janitor-batch-size` | `OBSCURA_NOTIFICATIONS_JANITOR_BATCH_SIZE` | `50` | Maximum number of invalid tokens to delete in a single batch. |
| `--janitor-channel-capacity` | `OBSCURA_NOTIFICATIONS_JANITOR_CHANNEL_CAPACITY` | `256` | Capacity of the invalid token janitor channel. |

## Storage (S3)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--storage-bucket` | `OBSCURA_STORAGE_BUCKET` | `obscura-attachments` | S3 bucket name for storing encrypted attachments. |
| `--storage-region` | `OBSCURA_STORAGE_REGION" | `us-east-1` | S3 region where the bucket is located. |
| `--storage-endpoint` | `OBSCURA_STORAGE_ENDPOINT` | None | Custom endpoint URL for S3-compatible services like MinIO. |
| `--storage-access-key` | `OBSCURA_STORAGE_ACCESS_KEY` | None | S3 access key ID. |
| `--storage-secret-key` | `OBSCURA_STORAGE_SECRET_KEY` | None | S3 secret access key. |
| `--storage-force-path-style` | `OBSCURA_STORAGE_FORCE_PATH_STYLE` | `false` | Whether to force path-style S3 URLs (required for MinIO). |
| `--storage-max-size-bytes` | `OBSCURA_STORAGE_MAX_SIZE_BYTES` | `52428800` | Maximum allowed size for a single attachment in bytes (50MB). |
| `--storage-cleanup-interval-secs` | `OBSCURA_STORAGE_CLEANUP_INTERVAL_SECS` | `3600` | How often to run the attachment cleanup task in seconds. |
| `--storage-cleanup-batch-size` | `OBSCURA_STORAGE_CLEANUP_BATCH_SIZE` | `100` | Maximum number of attachments to delete in a single batch. |

## WebSockets

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--ws-outbound-buffer-size` | `OBSCURA_WS_OUTBOUND_BUFFER_SIZE` | `32` | Capacity of the outbound WebSocket message buffer. |
| `--ws-ack-buffer-size` | `OBSCURA_WS_ACK_BUFFER_SIZE` | `1000` | Capacity of the message acknowledgment buffer. |
| `--ws-ack-batch-size` | `OBSCURA_WS_ACK_BATCH_SIZE` | `100` | Number of acknowledgments to batch before database deletion. |
| `--ws-ack-flush-interval-ms` | `OBSCURA_WS_ACK_FLUSH_INTERVAL_MS` | `500` | Interval in milliseconds to flush pending ACKs to the database. |
| `--ws-ping-interval-secs` | `OBSCURA_WS_PING_INTERVAL_SECS` | `30` | WebSocket heartbeat interval in seconds (A value of 0 results in a 1-second interval). |
| `--ws-ping-timeout-secs` | `OBSCURA_WS_PING_TIMEOUT_SECS` | `10` | Wait time for a pong response before closing the connection. |

## Health Checks

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--db-timeout-ms` | `OBSCURA_HEALTH_DB_TIMEOUT_MS` | `2000` | Timeout for the database health check in milliseconds. |
| `--storage-timeout-ms` | `OBSCURA_HEALTH_STORAGE_TIMEOUT_MS` | `2000` | Timeout for the storage health check in milliseconds. |
| `--pubsub-timeout-ms` | `OBSCURA_HEALTH_PUBSUB_TIMEOUT_MS` | `2000` | Timeout for the PubSub health check in milliseconds. |

## Telemetry

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--otlp-endpoint` | `OBSCURA_TELEMETRY_OTLP_ENDPOINT` | None | OTLP gRPC endpoint for exporting traces and metrics. |
| `--log-format` | `OBSCURA_TELEMETRY_LOG_FORMAT` | `text` | Log output format: `text` or `json`. |
| `--trace-sampling-ratio` | `OBSCURA_TELEMETRY_TRACE_SAMPLING_RATIO` | `1.0` | Ratio of traces to sample (1.0 = 100%). |
| `--metrics-export-interval-secs` | `OBSCURA_TELEMETRY_METRICS_EXPORT_INTERVAL_SECS` | `60` | Frequency of OTLP metric exports in seconds. |
| `--export-timeout-secs` | `OBSCURA_TELEMETRY_EXPORT_TIMEOUT_SECS` | `10` | Timeout for OTLP export requests in seconds. |
