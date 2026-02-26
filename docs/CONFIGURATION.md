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
| `--server-mgmt-port` | `OBSCURA_SERVER_MGMT_PORT` | `9090` | Management port for health checks and metrics. |
| `--server-shutdown-timeout-secs` | `OBSCURA_SERVER_SHUTDOWN_TIMEOUT_SECS` | `5` | How long to wait for background tasks to finish during shutdown in seconds. |
| `--server-request-timeout-secs` | `OBSCURA_SERVER_REQUEST_TIMEOUT_SECS` | `30` | Timeout for standard API requests in seconds. |
| `--server-global-timeout-secs` | `OBSCURA_SERVER_GLOBAL_TIMEOUT_SECS` | `600` | Global catch-all safety timeout for all requests in seconds. |
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
| `--auth-jwt-secret` | `OBSCURA_AUTH_JWT_SECRET` | `change_me_in_production` | Secret key for signing JWT access tokens. |
| `--auth-token-ttl-secs` | `OBSCURA_AUTH_TOKEN_TTL_SECS` | `900` | Access token time-to-live in seconds. |
| `--auth-refresh-token-ttl-days` | `OBSCURA_AUTH_REFRESH_TOKEN_TTL_DAYS` | `30` | Refresh token time-to-live in days. |
| `--auth-refresh-token-cleanup-interval-secs` | `OBSCURA_AUTH_REFRESH_TOKEN_CLEANUP_INTERVAL_SECS` | `86400` | How often to run the refresh token cleanup task in seconds. |

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
| `--messaging-inbox-max-size` | `OBSCURA_MESSAGING_INBOX_MAX_SIZE` | `1000` | Maximum number of pending messages per user before pruning. |
| `--messaging-cleanup-interval-secs` | `OBSCURA_MESSAGING_CLEANUP_INTERVAL_SECS` | `300` | How often to run the message cleanup task in seconds. |
| `--messaging-send-batch-limit` | `OBSCURA_MESSAGING_SEND_BATCH_LIMIT` | `100` | Maximum number of messages to accept in a single send request. |
| `--pre-key-refill-threshold` | `OBSCURA_PRE_KEY_REFILL_THRESHOLD` | `20` | Threshold of one-time prekeys to trigger a refill notification. |
| `--pre-keys-max` | `OBSCURA_PRE_KEYS_MAX` | `100` | Maximum number of one-time prekeys allowed per user. |

## Notifications

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--notifications-cleanup-interval-secs` | `OBSCURA_NOTIFICATIONS_CLEANUP_INTERVAL_SECS` | `60` | How often to run the notification channel cleanup. |
| `--notifications-global-channel-capacity` | `OBSCURA_NOTIFICATIONS_GLOBAL_CHANNEL_CAPACITY` | `1024` | Capacity of the global notification dispatcher channel. |
| `--notifications-user-channel-capacity` | `OBSCURA_NOTIFICATIONS_USER_CHANNEL_CAPACITY` | `64` | Capacity of the per-user notification channel. |
| `--notifications-push-delay-secs` | `OBSCURA_NOTIFICATIONS_PUSH_DELAY_SECS` | `2` | Delay in seconds before a push notification is sent as a fallback. |
| `--notifications-worker-interval-secs` | `OBSCURA_NOTIFICATIONS_WORKER_INTERVAL_SECS` | `1` | Interval in seconds for the notification worker to poll for jobs. |
| `--notifications-worker-concurrency` | `OBSCURA_NOTIFICATIONS_WORKER_CONCURRENCY` | `100` | Maximum concurrent push delivery tasks. |
| `--notifications-push-queue-key` | `OBSCURA_NOTIFICATIONS_PUSH_QUEUE_KEY` | `jobs:push_notifications` | Redis key for the push notification job queue. |
| `--notifications-channel-prefix` | `OBSCURA_NOTIFICATIONS_CHANNEL_PREFIX` | `user:` | Redis PubSub channel prefix for user notifications. |
| `--notifications-visibility-timeout-secs` | `OBSCURA_NOTIFICATIONS_VISIBILITY_TIMEOUT_SECS` | `30` | How long a push job is leased by a worker in seconds. |
| `--notifications-invalid-token-cleanup-interval-secs` | `OBSCURA_NOTIFICATIONS_INVALID_TOKEN_CLEANUP_INTERVAL_SECS` | `5` | How often invalid tokens are flushed to the database. |
| `--notifications-invalid-token-cleanup-batch-size` | `OBSCURA_NOTIFICATIONS_INVALID_TOKEN_CLEANUP_BATCH_SIZE` | `50` | Maximum number of invalid tokens to delete in a single batch. |
| `--notifications-invalid-token-cleanup-channel-capacity` | `OBSCURA_NOTIFICATIONS_INVALID_TOKEN_CLEANUP_CHANNEL_CAPACITY` | `256` | Capacity of the invalid token cleanup channel. |

## Attachments

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--attachment-prefix` | `OBSCURA_ATTACHMENT_PREFIX` | `attachments/` | S3 prefix for logical namespacing of attachments. |
| `--attachment-max-size-bytes` | `OBSCURA_ATTACHMENT_MAX_SIZE_BYTES` | `52428800` | Maximum allowed size for a single attachment in bytes (50MB). |
| `--attachment-min-size-bytes` | `OBSCURA_ATTACHMENT_MIN_SIZE_BYTES` | `1` | Minimum allowed size for a single attachment in bytes. |
| `--attachment-timeout-secs` | `OBSCURA_ATTACHMENT_TIMEOUT_SECS` | `120` | S3 streaming timeout for attachments in seconds. |
| `--attachment-cleanup-interval-secs` | `OBSCURA_ATTACHMENT_CLEANUP_INTERVAL_SECS` | `3600` | How often to run the attachment cleanup task in seconds. |
| `--attachment-cleanup-batch-size` | `OBSCURA_ATTACHMENT_CLEANUP_BATCH_SIZE` | `100` | Maximum number of attachments to delete in a single batch. |

## Backups

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--backup-prefix` | `OBSCURA_BACKUP_PREFIX` | `backups/` | S3 prefix for logical namespacing of backups. |
| `--backup-max-size-bytes` | `OBSCURA_BACKUP_MAX_SIZE_BYTES` | `2097152` | Max backup size in bytes (2MB). |
| `--backup-min-size-bytes` | `OBSCURA_BACKUP_MIN_SIZE_BYTES` | `32` | Min backup size in bytes to prevent accidental wipes. |
| `--backup-timeout-secs` | `OBSCURA_BACKUP_TIMEOUT_SECS` | `60` | S3 streaming timeout in seconds. |
| `--backup-stale-threshold-mins` | `OBSCURA_BACKUP_STALE_THRESHOLD_MINS` | `30` | Grace period for "UPLOADING" state before cleanup. |
| `--backup-cleanup-interval-secs` | `OBSCURA_BACKUP_CLEANUP_INTERVAL_SECS` | `300` | Frequency of background cleanup worker cycles. |

## Storage (S3 Infrastructure)

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--storage-bucket` | `OBSCURA_STORAGE_BUCKET` | `obscura-storage` | S3 bucket name for object storage. |
| `--storage-region` | `OBSCURA_STORAGE_REGION` | `us-east-1` | S3 region where the bucket is located. |
| `--storage-endpoint` | `OBSCURA_STORAGE_ENDPOINT` | None | Custom endpoint URL for S3-compatible services like MinIO. |
| `--storage-access-key` | `OBSCURA_STORAGE_ACCESS_KEY` | None | S3 access key ID. |
| `--storage-secret-key` | `OBSCURA_STORAGE_SECRET_KEY` | None | S3 secret access key. |
| `--storage-force-path-style` | `OBSCURA_STORAGE_FORCE_PATH_STYLE` | `false` | Whether to force path-style S3 URLs (required for MinIO). |

## WebSockets

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--ws-outbound-buffer-size` | `OBSCURA_WS_OUTBOUND_BUFFER_SIZE` | `32` | Capacity of the outbound WebSocket message buffer. |
| `--ws-ack-buffer-size` | `OBSCURA_WS_ACK_BUFFER_SIZE` | `1000` | Capacity of the message acknowledgment buffer. |
| `--ws-ack-batch-size` | `OBSCURA_WS_ACK_BATCH_SIZE` | `100` | Number of acknowledgments to batch before database deletion. |
| `--ws-ack-flush-interval-ms` | `OBSCURA_WS_ACK_FLUSH_INTERVAL_MS` | `500` | Interval in milliseconds to flush pending ACKs to the database. |
| `--ws-ping-interval-secs` | `OBSCURA_WS_PING_INTERVAL_SECS` | `30` | WebSocket heartbeat interval in seconds (A value of 0 results in a 1-second interval). |
| `--ws-ping-timeout-secs` | `OBSCURA_WS_PING_TIMEOUT_SECS` | `10` | Wait time for a pong response before closing the connection. |
| `--ws-message-fetch-batch-size` | `OBSCURA_WS_MESSAGE_FETCH_BATCH_SIZE` | `50` | Maximum number of messages to fetch in a single database query loop. |

## Health Checks

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--health-db-timeout-ms` | `OBSCURA_HEALTH_DB_TIMEOUT_MS` | `2000` | Timeout for the database health check in milliseconds. |
| `--health-storage-timeout-ms` | `OBSCURA_HEALTH_STORAGE_TIMEOUT_MS` | `2000` | Timeout for the storage health check in milliseconds. |
| `--health-pubsub-timeout-ms` | `OBSCURA_HEALTH_PUBSUB_TIMEOUT_MS` | `2000` | Timeout for the PubSub health check in milliseconds. |

## Telemetry

| Flag | Environment Variable | Default | Description |
|------|----------------------|---------|-------------|
| `--telemetry-otlp-endpoint` | `OBSCURA_TELEMETRY_OTLP_ENDPOINT` | None | OTLP gRPC endpoint for exporting traces and metrics. |
| `--telemetry-log-format` | `OBSCURA_TELEMETRY_LOG_FORMAT` | `text` | Log output format: `text` or `json`. |
| `--telemetry-trace-sampling-ratio` | `OBSCURA_TELEMETRY_TRACE_SAMPLING_RATIO` | `1.0` | Ratio of traces to sample (1.0 = 100%). |
| `--telemetry-metrics-export-interval-secs` | `OBSCURA_TELEMETRY_METRICS_EXPORT_INTERVAL_SECS` | `60` | Frequency of OTLP metric exports in seconds. |
| `--telemetry-export-timeout-secs` | `OBSCURA_TELEMETRY_EXPORT_TIMEOUT_SECS` | `10` | Timeout for OTLP export requests in seconds. |
