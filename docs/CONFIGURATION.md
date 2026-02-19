# Configuration Reference

Obscura Server can be configured using either command-line options or environment variables. Command-line options take precedence.

## General

| Option | Description | Environment Variable | Default | Required |
|--------|-------------|----------------------|---------|----------|
| `--ttl-days` | Global days before messages/attachments are auto-deleted | `OBSCURA_TTL_DAYS` | `30` | No |

## Database

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--db-url` | PostgreSQL connection string | `OBSCURA_DATABASE_URL` | `postgres://user:password@localhost/signal_server` |
| `--db-max-connections` | Max connections in the pool | `OBSCURA_DATABASE_MAX_CONNECTIONS` | `20` |
| `--db-min-connections` | Min connections to keep idle | `OBSCURA_DATABASE_MIN_CONNECTIONS` | `5` |
| `--db-acquire-timeout-secs` | Seconds to wait for a connection | `OBSCURA_DATABASE_ACQUIRE_TIMEOUT_SECS` | `3` |
| `--db-idle-timeout-secs` | Seconds before an idle connection closes | `OBSCURA_DATABASE_IDLE_TIMEOUT_SECS` | `600` |
| `--db-max-lifetime-secs` | Seconds before a connection is recycled | `OBSCURA_DATABASE_MAX_LIFETIME_SECS` | `1800` |

## Server

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--server-host` | Interface to bind the server to | `OBSCURA_SERVER_HOST` | `0.0.0.0` |
| `--server-port` | Port to bind the server to | `OBSCURA_SERVER_PORT` | `3000` |
| `--mgmt-port` | Management port for health/metrics | `OBSCURA_SERVER_MGMT_PORT` | `9090` |
| `--shutdown-timeout-secs` | Timeout for background tasks during shutdown | `OBSCURA_SERVER_SHUTDOWN_TIMEOUT_SECS` | `5` |
| `--trusted-proxies` | CIDR ranges of trusted proxies | `OBSCURA_SERVER_TRUSTED_PROXIES` | `10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.1/32` |

## Authentication

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--jwt-secret` | Secret key for signing JWTs | `OBSCURA_AUTH_JWT_SECRET` | `change_me_in_production` |
| `--access-token-ttl-secs` | Access token time-to-live in seconds | `OBSCURA_AUTH_TOKEN_TTL_SECS" | `900` |
| `--refresh-token-ttl-days` | Refresh token time-to-live in days | `OBSCURA_AUTH_REFRESH_TOKEN_TTL_DAYS` | `30` |

## Rate Limiting

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--rate-limit-per-second` | API requests allowed per second | `OBSCURA_RATE_LIMIT_PER_SECOND` | `10` |
| `--rate-limit-burst` | Max API burst allowance per IP | `OBSCURA_RATE_LIMIT_BURST` | `20` |
| `--auth-rate-limit-per-second` | Auth requests allowed per second | `OBSCURA_RATE_LIMIT_AUTH_PER_SECOND` | `1` |
| `--auth-rate-limit-burst` | Auth API burst allowance per IP | `OBSCURA_RATE_LIMIT_AUTH_BURST` | `3` |

## Health Checking

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--db-timeout-ms` | Timeout for DB health check | `OBSCURA_HEALTH_DB_TIMEOUT_MS` | `2000` |
| `--storage-timeout-ms` | Timeout for storage health check | `OBSCURA_HEALTH_STORAGE_TIMEOUT_MS` | `2000` |
| `--pubsub-timeout-ms` | Timeout for PubSub health check | `OBSCURA_HEALTH_PUBSUB_TIMEOUT_MS` | `2000` |

## Messaging

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--max-inbox-size` | Max pending messages per user | `OBSCURA_MESSAGING_INBOX_MAX_SIZE` | `1000` |
| `--messaging-cleanup-interval-secs` | How often to run message cleanup | `OBSCURA_MESSAGING_CLEANUP_INTERVAL_SECS` | `300` |
| `--batch-limit` | Max messages sent per DB fetch loop | `OBSCURA_MESSAGING_BATCH_LIMIT` | `50` |
| `--pre-key-refill-threshold` | Threshold to trigger client refill notification | `OBSCURA_PRE_KEY_REFILL_THRESHOLD` | `20` |
| `--max-pre-keys` | Max One-Time PreKeys allowed per user | `OBSCURA_PRE_KEYS_MAX` | `100` |

## Notifications

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--gc-interval-secs` | How often to run notification cleanup | `OBSCURA_NOTIFICATIONS_GC_INTERVAL_SECS` | `60` |
| `--global-channel-capacity` | Capacity of global dispatcher channel | `OBSCURA_NOTIFICATIONS_GLOBAL_CHANNEL_CAPACITY` | `1024` |
| `--user-channel-capacity" | Capacity of per-user notification channel | `OBSCURA_NOTIFICATIONS_USER_CHANNEL_CAPACITY` | `64` |
| `--push-delay-secs` | Grace period before sending a fallback push | `OBSCURA_NOTIFICATIONS_PUSH_DELAY_SECS` | `2` |
| `--worker-interval-secs` | Frequency of push queue polling | `OBSCURA_NOTIFICATIONS_WORKER_INTERVAL_SECS` | `1` |
| `--worker-concurrency` | Max concurrent push delivery tasks (also Redis poll limit) | `OBSCURA_NOTIFICATIONS_WORKER_CONCURRENCY` | `100` |
| `--push-queue-key` | Redis key for the push notification queue | `OBSCURA_NOTIFICATIONS_PUSH_QUEUE_KEY` | `jobs:push_notifications` |
| `--channel-prefix` | Redis PubSub prefix for user pokes | `OBSCURA_NOTIFICATIONS_CHANNEL_PREFIX` | `user:` |
| `--visibility-timeout-secs` | How long a push job is leased by a worker | `OBSCURA_NOTIFICATIONS_VISIBILITY_TIMEOUT_SECS` | `30` |
| `--janitor-interval-secs` | Frequency of invalid token cleanup flushes | `OBSCURA_NOTIFICATIONS_JANITOR_INTERVAL_SECS" | `5` |
| `--janitor-batch-size` | Max invalid tokens deleted per batch | `OBSCURA_NOTIFICATIONS_JANITOR_BATCH_SIZE" | `50` |
| `--janitor-channel-capacity` | Capacity of the janitor's token buffer | `OBSCURA_NOTIFICATIONS_JANITOR_CHANNEL_CAPACITY` | `256` |

## PubSub

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--pubsub-url` | PubSub connection URL | `OBSCURA_PUBSUB_URL` | `redis://localhost:6379` |
| `--pubsub-min-backoff-secs` | Min reconnection backoff | `OBSCURA_PUBSUB_MIN_BACKOFF_SECS` | `1` |
| `--pubsub-max-backoff-secs" | Max reconnection backoff | `OBSCURA_PUBSUB_MAX_BACKOFF_SECS" | `30` |

## WebSockets

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--ws-outbound-buffer-size` | WS outbound channel capacity | `OBSCURA_WS_OUTBOUND_BUFFER_SIZE` | `32` |
| `--ws-ack-buffer-size` | WS ACK channel capacity | `OBSCURA_WS_ACK_BUFFER_SIZE" | `1000` |
| `--ws-ack-batch-size` | WS ACK DB batch size | `OBSCURA_WS_ACK_BATCH_SIZE" | `100` |
| `--ws-ack-flush-interval-ms` | WS ACK DB flush interval | `OBSCURA_WS_ACK_FLUSH_INTERVAL_MS" | `500` |
| `--ws-ping-interval-secs` | How often to send a WebSocket ping frame in seconds (0 to disable) | `OBSCURA_WS_PING_INTERVAL_SECS" | `30` |
| `--ws-ping-timeout-secs` | How long to wait for a pong response before closing the connection in seconds | `OBSCURA_WS_PING_TIMEOUT_SECS" | `10` |

## Storage

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--storage-bucket` | Storage Bucket Name | `OBSCURA_STORAGE_BUCKET` | `obscura-attachments` |
| `--storage-region` | Storage Region | `OBSCURA_STORAGE_REGION` | `us-east-1` |
| `--storage-endpoint` | Custom Storage Endpoint (e.g., for MinIO) | `OBSCURA_STORAGE_ENDPOINT" | - |
| `--storage-access-key` | Storage Access Key ID | `OBSCURA_STORAGE_ACCESS_KEY" | - |
| `--storage-secret-key` | Storage Secret Access Key | `OBSCURA_STORAGE_SECRET_KEY" | - |
| `--storage-force-path-style` | Force Path Style (Required for MinIO) | `OBSCURA_STORAGE_FORCE_PATH_STYLE` | `false` |
| `--storage-max-size-bytes` | Max attachment size in bytes | `OBSCURA_STORAGE_MAX_SIZE_BYTES" | `52428800` |
| `--storage-cleanup-interval-secs` | How often to run attachment cleanup | `OBSCURA_STORAGE_CLEANUP_INTERVAL_SECS" | `3600` |
| `--storage-cleanup-batch-size` | Max attachments to delete per batch | `OBSCURA_STORAGE_CLEANUP_BATCH_SIZE" | `100` |

## Telemetry

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--otlp-endpoint` | OTLP endpoint for traces/metrics | `OBSCURA_TELEMETRY_OTLP_ENDPOINT" | - |
| `--log-format` | Log format (`text` or `json`) | `OBSCURA_TELEMETRY_LOG_FORMAT` | `text` |
| `--trace-sampling-ratio` | Trace sampling ratio (0.0 to 1.0) | `OBSCURA_TELEMETRY_TRACE_SAMPLING_RATIO` | `1.0` |
| `--metrics-export-interval-secs` | How often to export metrics | `OBSCURA_TELEMETRY_METRICS_EXPORT_INTERVAL_SECS" | `60` |
| `--export-timeout-secs` | OTLP export timeout | `OBSCURA_TELEMETRY_EXPORT_TIMEOUT_SECS" | `10` |
