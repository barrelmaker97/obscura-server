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
| `--shutdown-timeout-secs` | `OBSCURA_SERVER_SHUTDOWN_TIMEOUT_SECS` | `5` | Grace period for background tasks during shutdown. |
| `--trusted-proxies` | `OBSCURA_SERVER_TRUSTED_PROXIES` | `10.0.0.0/8,...` | CIDR ranges trusted for `X-Forwarded-For` extraction. |

## Database (PostgreSQL)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--db-url` | `OBSCURA_DATABASE_URL` | `postgres://...` | PostgreSQL connection string. |
| `--db-max-connections` | `OBSCURA_DATABASE_MAX_CONNECTIONS` | `20` | Maximum connections in the pool. |
| `--db-min-connections` | `OBSCURA_DATABASE_MIN_CONNECTIONS` | `5` | Minimum idle connections to maintain. |
| `--db-acquire-timeout-secs` | `OBSCURA_DATABASE_ACQUIRE_TIMEOUT_SECS` | `3` | Seconds to wait for a connection from the pool. |
| `--db-idle-timeout-secs` | `OBSCURA_DATABASE_IDLE_TIMEOUT_SECS` | `600` | Seconds before an idle connection is closed. |
| `--db-max-lifetime-secs` | `OBSCURA_DATABASE_MAX_LIFETIME_SECS` | `1800` | Seconds before a connection is retired. |

## PubSub (Redis/Valkey)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--pubsub-url` | `OBSCURA_PUBSUB_URL` | `redis://localhost:6379` | Connection URL for the PubSub/Job backend. |
| `--pubsub-min-backoff-secs` | `OBSCURA_PUBSUB_MIN_BACKOFF_SECS` | `1` | Min backoff for PubSub reconnection. |
| `--pubsub-max-backoff-secs` | `OBSCURA_PUBSUB_MAX_BACKOFF_SECS` | `30` | Max backoff for PubSub reconnection. |

## Authentication

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--jwt-secret` | `OBSCURA_AUTH_JWT_SECRET` | `change_me...` | Secret key for signing JWT access tokens. |
| `--access-token-ttl-secs` | `OBSCURA_AUTH_TOKEN_TTL_SECS` | `900` | Access token (JWT) validity in seconds. |
| `--refresh-token-ttl-days` | `OBSCURA_AUTH_REFRESH_TOKEN_TTL_DAYS` | `30` | Refresh token validity in days. |

## Rate Limiting

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--rate-limit-per-second` | `OBSCURA_RATE_LIMIT_PER_SECOND` | `10` | Standard API requests allowed per second per IP. |
| `--rate-limit-burst` | `OBSCURA_RATE_LIMIT_BURST` | `20` | Standard API burst allowance. |
| `--auth-rate-limit-per-second` | `OBSCURA_RATE_LIMIT_AUTH_PER_SECOND` | `1` | Stricter limit for login/register per second. |
| `--auth-rate-limit-burst` | `OBSCURA_RATE_LIMIT_AUTH_BURST` | `3` | Stricter burst for login/register. |

## Messaging & Keys

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--max-inbox-size` | `OBSCURA_MESSAGING_INBOX_MAX_SIZE` | `1000` | Max pending messages per user before pruning. |
| `--messaging-cleanup-interval-secs` | `OBSCURA_MESSAGING_CLEANUP_INTERVAL_SECS` | `300` | Frequency of the message cleanup task. |
| `--batch-limit` | `OBSCURA_MESSAGING_BATCH_LIMIT` | `50` | Max messages processed per DB fetch loop. |
| `--pre-key-refill-threshold` | `OBSCURA_PRE_KEY_REFILL_THRESHOLD` | `20` | Threshold to trigger a refill notification. |
| `--max-pre-keys` | `OBSCURA_PRE_KEYS_MAX` | `100` | Max One-Time PreKeys allowed per user. |

## Notifications

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--gc-interval-secs` | `OBSCURA_NOTIFICATIONS_GC_INTERVAL_SECS` | `60` | Frequency of notification channel cleanup. |
| `--push-delay-secs` | `OBSCURA_NOTIFICATIONS_PUSH_DELAY_SECS` | `2` | Delay before sending a fallback push notification. |
| `--worker-interval-secs` | `OBSCURA_NOTIFICATIONS_WORKER_INTERVAL_SECS` | `1` | Frequency of push queue polling. |
| `--worker-concurrency` | `OBSCURA_NOTIFICATIONS_WORKER_CONCURRENCY` | `100` | Max concurrent push delivery tasks. |
| `--visibility-timeout-secs` | `OBSCURA_NOTIFICATIONS_VISIBILITY_TIMEOUT_SECS` | `30` | How long a push job is leased by a worker. |
| `--janitor-interval-secs` | `OBSCURA_NOTIFICATIONS_JANITOR_INTERVAL_SECS` | `5` | Frequency of invalid token flushes to DB. |

## Storage (S3)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--storage-bucket` | `OBSCURA_STORAGE_BUCKET` | `obscura-attachments` | S3 bucket name. |
| `--storage-region` | `OBSCURA_STORAGE_REGION` | `us-east-1` | S3 region. |
| `--storage-endpoint` | `OBSCURA_STORAGE_ENDPOINT` | - | Custom endpoint (e.g., for MinIO). |
| `--storage-access-key` | `OBSCURA_STORAGE_ACCESS_KEY` | - | S3 Access Key ID. |
| `--storage-secret-key` | `OBSCURA_STORAGE_SECRET_KEY` | - | S3 Secret Access Key. |
| `--storage-max-size-bytes` | `OBSCURA_STORAGE_MAX_SIZE_BYTES` | `52428800` | Max attachment size (Default 50MB). |
| `--storage-cleanup-interval-secs` | `OBSCURA_STORAGE_CLEANUP_INTERVAL_SECS` | `3600` | Frequency of attachment cleanup tasks. |

## WebSockets

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--ws-outbound-buffer-size` | `OBSCURA_WS_OUTBOUND_BUFFER_SIZE` | `32` | Outbound message channel capacity. |
| `--ws-ack-buffer-size` | `OBSCURA_WS_ACK_BUFFER_SIZE` | `1000` | Capacity of the ACK processing buffer. |
| `--ws-ack-batch-size` | `OBSCURA_WS_ACK_BATCH_SIZE` | `100` | Number of ACKs to batch for DB deletion. |
| `--ws-ack-flush-interval-ms` | `OBSCURA_WS_ACK_FLUSH_INTERVAL_MS` | `500` | Interval to flush pending ACKs to the DB. |
| `--ws-ping-interval-secs` | `OBSCURA_WS_PING_INTERVAL_SECS` | `30` | WebSocket heartbeat interval (0 to disable). |
| `--ws-ping-timeout-secs` | `OBSCURA_WS_PING_TIMEOUT_SECS` | `10` | Wait time for pong before closing connection. |

## Health Checks

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--db-timeout-ms` | `OBSCURA_HEALTH_DB_TIMEOUT_MS` | `2000` | Timeout for database health check. |
| `--storage-timeout-ms` | `OBSCURA_HEALTH_STORAGE_TIMEOUT_MS` | `2000` | Timeout for storage health check. |
| `--pubsub-timeout-ms` | `OBSCURA_HEALTH_PUBSUB_TIMEOUT_MS` | `2000` | Timeout for PubSub health check. |

## Telemetry

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--otlp-endpoint` | `OBSCURA_TELEMETRY_OTLP_ENDPOINT` | - | OTLP gRPC endpoint for traces/metrics. |
| `--log-format` | `OBSCURA_TELEMETRY_LOG_FORMAT` | `text` | Log output format (`text` or `json`). |
| `--trace-sampling-ratio` | `OBSCURA_TELEMETRY_TRACE_SAMPLING_RATIO` | `1.0` | Ratio of traces to sample (0.0 to 1.0). |
| `--metrics-export-interval-secs` | `OBSCURA_TELEMETRY_METRICS_EXPORT_INTERVAL_SECS` | `60` | Frequency of OTLP metric exports. |
