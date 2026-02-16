# Obscura Server
**Obscura Server** is a minimalist, secure relay server for the Signal Protocol. It facilitates end-to-end encrypted asynchronous messaging while knowing nothing about the content of the messages.

## Features

- **Zero-Knowledge Architecture**: The server stores opaque encrypted blobs. It cannot read message content.
- **Signal Protocol Support**: Uses PreKeys (Identity, Signed, One-Time) to enable X3DH key exchanges.
- **Asynchronous Delivery**: Stores encrypted messages until the recipient comes online to fetch them.
- **Encrypted Attachments**: Supports secure file uploads via S3-compatible storage with automated expiry.
- **Strict Privacy Limits**: Automatic garbage collection of old messages and global inbox limits to prevent metadata buildup.
- **OpenTelemetry Native**: Built-in support for Traces, Metrics, and Structured Logging via OTLP.
- **Container Native**: Built with Docker in mind for easy deployment.
- **Configurable**: Fully configurable via command-line flags or environment variables.

## Configuration

Obscura Server can be configured using either command-line options or by setting corresponding environment variables.
Command-line options take precedence over environment variables.

### General Configuration

| Option | Description | Environment Variable | Default | Required |
|--------|-------------|----------------------|---------|----------|
| `--database-url` | PostgreSQL connection string | `OBSCURA_DATABASE_URL` | `postgres://user:password@localhost/signal_server` | No |
| `--ttl-days` | Global days before messages/attachments are auto-deleted | `OBSCURA_TTL_DAYS` | `30` | No |

### Server Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--server-host` | Interface to bind the server to | `OBSCURA_SERVER_HOST` | `0.0.0.0` |
| `--server-port` | Port to bind the server to | `OBSCURA_SERVER_PORT` | `3000` |
| `--mgmt-port` | Management port for health/metrics | `OBSCURA_SERVER_MGMT_PORT` | `9090` |
| `--shutdown-timeout-secs` | Timeout for background tasks during shutdown | `OBSCURA_SERVER_SHUTDOWN_TIMEOUT_SECS` | `5` |
| `--trusted-proxies` | CIDR ranges of trusted proxies | `OBSCURA_SERVER_TRUSTED_PROXIES` | `10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.1/32` |

### Auth Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--jwt-secret` | Secret key for signing JWTs | `OBSCURA_AUTH_JWT_SECRET` | `change_me_in_production` |
| `--access-token-ttl-secs` | Access token time-to-live in seconds | `OBSCURA_AUTH_TOKEN_TTL_SECS` | `900` |
| `--refresh-token-ttl-days` | Refresh token time-to-live in days | `OBSCURA_AUTH_REFRESH_TOKEN_TTL_DAYS` | `30` |

### Rate Limiting

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--rate-limit-per-second` | API requests allowed per second | `OBSCURA_RATE_LIMIT_PER_SECOND` | `10` |
| `--rate-limit-burst` | Max API burst allowance per IP | `OBSCURA_RATE_LIMIT_BURST` | `20` |
| `--auth-rate-limit-per-second` | Auth requests allowed per second | `OBSCURA_RATE_LIMIT_AUTH_PER_SECOND` | `1` |
| `--auth-rate-limit-burst` | Auth API burst allowance per IP | `OBSCURA_RATE_LIMIT_AUTH_BURST` | `3` |

### Health Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--db-timeout-ms` | Timeout for DB health check | `OBSCURA_HEALTH_DB_TIMEOUT_MS` | `2000` |
| `--storage-timeout-ms` | Timeout for storage health check | `OBSCURA_HEALTH_STORAGE_TIMEOUT_MS` | `2000` |
| `--pubsub-timeout-ms` | Timeout for PubSub health check | `OBSCURA_HEALTH_PUBSUB_TIMEOUT_MS` | `2000` |

### Messaging Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--max-inbox-size` | Max pending messages per user | `OBSCURA_MESSAGING_INBOX_MAX_SIZE` | `1000` |
| `--messaging-cleanup-interval-secs` | How often to run message cleanup | `OBSCURA_MESSAGING_CLEANUP_INTERVAL_SECS` | `300` |
| `--batch-limit` | Max messages sent per DB fetch loop | `OBSCURA_MESSAGING_BATCH_LIMIT` | `50` |
| `--pre-key-refill-threshold` | Threshold to trigger client refill notification | `OBSCURA_PRE_KEY_REFILL_THRESHOLD` | `20` |
| `--max-pre-keys` | Max One-Time PreKeys allowed per user | `OBSCURA_PRE_KEYS_MAX` | `100` |

### Notification Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--gc-interval-secs` | How often to run notification cleanup | `OBSCURA_NOTIFICATIONS_GC_INTERVAL_SECS` | `60` |
| `--global-channel-capacity` | Capacity of global dispatcher channel | `OBSCURA_NOTIFICATIONS_GLOBAL_CHANNEL_CAPACITY` | `1024` |
| `--user-channel-capacity` | Capacity of per-user notification channel | `OBSCURA_NOTIFICATIONS_USER_CHANNEL_CAPACITY` | `64` |
| `--push-delay-secs` | Grace period before sending a fallback push | `OBSCURA_NOTIFICATIONS_PUSH_DELAY_SECS` | `10` |
| `--worker-interval-secs` | Frequency of push queue polling | `OBSCURA_NOTIFICATIONS_WORKER_INTERVAL_SECS` | `1` |
| `--worker-concurrency` | Max concurrent push delivery tasks (also Redis poll limit) | `OBSCURA_NOTIFICATIONS_WORKER_CONCURRENCY` | `100` |
| `--push-queue-key` | Redis key for the push notification queue | `OBSCURA_NOTIFICATIONS_PUSH_QUEUE_KEY` | `jobs:push_notifications` |
| `--channel-prefix` | Redis PubSub prefix for user pokes | `OBSCURA_NOTIFICATIONS_CHANNEL_PREFIX` | `user:` |
| `--visibility-timeout-secs` | How long a push job is leased by a worker | `OBSCURA_NOTIFICATIONS_VISIBILITY_TIMEOUT_SECS` | `30` |
| `--janitor-interval-secs` | Frequency of invalid token cleanup flushes | `OBSCURA_NOTIFICATIONS_JANITOR_INTERVAL_SECS` | `5` |
| `--janitor-batch-size` | Max invalid tokens deleted per batch | `OBSCURA_NOTIFICATIONS_JANITOR_BATCH_SIZE` | `50` |
| `--janitor-channel-capacity` | Capacity of the janitor's token buffer | `OBSCURA_NOTIFICATIONS_JANITOR_CHANNEL_CAPACITY` | `256` |

### PubSub Configuration (Distributed Notifications)

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--pubsub-url` | PubSub connection URL | `OBSCURA_PUBSUB_URL` | `redis://localhost:6379` |
| `--pubsub-min-backoff-secs` | Min reconnection backoff | `OBSCURA_PUBSUB_MIN_BACKOFF_SECS` | `1` |
| `--pubsub-max-backoff-secs` | Max reconnection backoff | `OBSCURA_PUBSUB_MAX_BACKOFF_SECS` | `30` |

### WebSocket Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--ws-outbound-buffer-size` | WS outbound channel capacity | `OBSCURA_WS_OUTBOUND_BUFFER_SIZE` | `32` |
| `--ws-ack-buffer-size` | WS ACK channel capacity | `OBSCURA_WS_ACK_BUFFER_SIZE` | `100` |
| `--ws-ack-batch-size` | WS ACK DB batch size | `OBSCURA_WS_ACK_BATCH_SIZE` | `50` |
| `--ws-ack-flush-interval-ms` | WS ACK DB flush interval | `OBSCURA_WS_ACK_FLUSH_INTERVAL_MS` | `500` |

### Storage Configuration (Attachments)

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--storage-bucket` | Storage Bucket Name | `OBSCURA_STORAGE_BUCKET` | `obscura-attachments` |
| `--storage-region` | Storage Region | `OBSCURA_STORAGE_REGION` | `us-east-1` |
| `--storage-endpoint` | Custom Storage Endpoint (e.g., for MinIO) | `OBSCURA_STORAGE_ENDPOINT` | - |
| `--storage-access-key` | Storage Access Key ID | `OBSCURA_STORAGE_ACCESS_KEY` | - |
| `--storage-secret-key` | Storage Secret Access Key | `OBSCURA_STORAGE_SECRET_KEY` | - |
| `--storage-force-path-style` | Force Path Style (Required for MinIO) | `OBSCURA_STORAGE_FORCE_PATH_STYLE` | `false` |
| `--storage-max-size-bytes` | Max attachment size in bytes | `OBSCURA_STORAGE_MAX_SIZE_BYTES` | `52428800` |
| `--storage-cleanup-interval-secs` | How often to run attachment cleanup | `OBSCURA_STORAGE_CLEANUP_INTERVAL_SECS` | `3600` |
| `--storage-cleanup-batch-size` | Max attachments to delete per batch | `OBSCURA_STORAGE_CLEANUP_BATCH_SIZE` | `100` |

### Telemetry Configuration

| Option | Description | Environment Variable | Default |
|--------|-------------|----------------------|---------|
| `--otlp-endpoint` | OTLP endpoint for traces/metrics | `OBSCURA_TELEMETRY_OTLP_ENDPOINT` | - |
| `--log-format` | Log format (`text` or `json`) | `OBSCURA_TELEMETRY_LOG_FORMAT` | `text` |
| `--trace-sampling-ratio` | Trace sampling ratio (0.0 to 1.0) | `OBSCURA_TELEMETRY_TRACE_SAMPLING_RATIO` | `1.0` |
| `--metrics-export-interval-secs` | How often to export metrics | `OBSCURA_TELEMETRY_METRICS_EXPORT_INTERVAL_SECS` | `60` |
| `--export-timeout-secs` | OTLP export timeout | `OBSCURA_TELEMETRY_EXPORT_TIMEOUT_SECS` | `10` |

### Example

```bash
# Using Flags
./obscura-server \
  --database-url postgres://user:pass@localhost/db \
  --jwt-secret my_secret \
  --storage-bucket my-attachments \
  --server-port 8080

# Using Environment Variables
export OBSCURA_DATABASE_URL=postgres://user:pass@localhost/db
export OBSCURA_AUTH_JWT_SECRET=my_secret
export OBSCURA_STORAGE_BUCKET=my-attachments
./obscura-server
```

## Docker

A Dockerfile is included for easy deployment.

### Build and Run

1. **Build the image**:
   ```bash
   docker build -t obscura-server .
   ```

2. **Run with Docker**:
   ```bash
   docker build -t obscura-server .
   docker run -d \
     -p 3000:3000 \
     -p 9090:9090 \
     -e OBSCURA_DATABASE_URL="postgres://user:pass@host.docker.internal:5432/obscura" \
     -e OBSCURA_PUBSUB_URL="redis://host.docker.internal:6379" \
     -e OBSCURA_AUTH_JWT_SECRET="your_secret_key" \
     -e OBSCURA_STORAGE_BUCKET="obscura-attachments" \
     obscura-server
   ```

### Docker Compose

A `docker-compose.yml` is provided for a complete local stack (Postgres + Valkey + MinIO + Server):

```bash
docker compose up -d
```

## Development

### Prerequisites
- Rust 1.83+
- PostgreSQL 16+
- Valkey 8+ (or Redis)
- `protoc` (Protocol Buffers compiler)

### Running Locally

1. Start Postgres, MinIO, and Valkey:
   ```bash
   docker compose up -d db minio valkey
   ```

2. Run the server:
   ```bash
   export OBSCURA_DATABASE_URL=postgres://user:password@localhost/signal_server
   export OBSCURA_PUBSUB_URL=redis://localhost:6379
   export OBSCURA_AUTH_JWT_SECRET=test
   export OBSCURA_STORAGE_BUCKET=test
   cargo run
   ```
   *Migrations are applied automatically on startup.*

### Testing

```bash
cargo test
```

## Releasing

Releases are managed via GitHub Actions.

1. Go to the **Actions** tab in the GitHub repository.
2. Select the **Bump Version & Tag** workflow on the left.
3. Click **Run workflow**.
4. Select the **Bump Type** from the dropdown (`patch`, `minor`, or `major`).
5. Click **Run workflow**.

The system will automatically:
1. Bump the version in `Cargo.toml` based on your selection.
2. Commit and Tag the release.
3. Trigger the **Publish Release** workflow to build and publish artifacts to Crates.io and GHCR.

## API Documentation

- **REST API**: Defined in `openapi.yaml`.
- **WebSocket**: Available at `/v1/gateway`. Expects Protobuf messages defined in `proto/obscura/v1/obscura.proto`.

# License

Copyright (c) 2026 Nolan Cooper

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.
