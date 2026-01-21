# Obscura Server
**Obscura Server** is a minimalist, secure relay server for the Signal Protocol. It facilitates end-to-end encrypted asynchronous messaging while knowing nothing about the content of the messages.

## Features

- **Zero-Knowledge Architecture**: The server stores opaque encrypted blobs. It cannot read message content.
- **Signal Protocol Support**: Uses PreKeys (Identity, Signed, One-Time) to enable X3DH key exchanges.
- **Asynchronous Delivery**: Stores encrypted messages until the recipient comes online to fetch them.
- **Strict Privacy Limits**: Automatic garbage collection of old messages and global inbox limits to prevent metadata buildup.
- **Container Native**: Built with Docker in mind for easy deployment.
- **Configurable**: Fully configurable via command-line flags or environment variables.

## Configuration

Obscura Server can be configured using either command-line options or by setting corresponding environment variables.
Command-line options take precedence over environment variables.

| Option | Description | Environment Variable | Default | Required |
|--------|-------------|----------------------|---------|----------|
| `--database-url` | PostgreSQL connection string | `OBSCURA_DATABASE_URL` | - | **Yes** |
| `--ttl-days` | Global days before messages/attachments are auto-deleted | `OBSCURA_TTL_DAYS` | `30` | No |
| `--jwt-secret` | Secret key for signing JWTs | `OBSCURA_JWT_SECRET` | - | **Yes** |
| `--host` | Interface to bind the server to | `OBSCURA_HOST` | `0.0.0.0` | No |
| `--port` | Port to bind the server to | `OBSCURA_PORT` | `3000` | No |
| `--access-token-ttl-secs` | Access Token lifetime in seconds | `OBSCURA_ACCESS_TOKEN_TTL_SECS` | `900` | No |
| `--refresh-token-ttl-days` | Refresh Token lifetime in days | `OBSCURA_REFRESH_TOKEN_TTL_DAYS` | `30` | No |
| `--max-inbox-size` | Max pending messages per user | `OBSCURA_MAX_INBOX_SIZE` | `1000` | No |
| `--batch-limit` | Max messages sent per DB fetch loop | `OBSCURA_BATCH_LIMIT` | `50` | No |
| `--per-second` | API requests allowed per second | `OBSCURA_RATE_LIMIT_PER_SECOND` | `10` | No |
| `--burst` | Max API burst allowance per IP | `OBSCURA_RATE_LIMIT_BURST` | `20` | No |
| `--auth-per-second` | Auth requests allowed per second | `OBSCURA_AUTH_RATE_LIMIT_PER_SECOND` | `1` | No |
| `--auth-burst` | Auth API burst allowance per IP | `OBSCURA_AUTH_RATE_LIMIT_BURST` | `3` | No |
| `--trusted-proxies` | CIDR ranges of trusted proxies | `OBSCURA_TRUSTED_PROXIES` | `10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.1/32` | No |
| `--outbound-buffer-size` | WS outbound channel capacity | `OBSCURA_WS_OUTBOUND_BUFFER_SIZE` | `32` | No |
| `--ack-buffer-size` | WS ACK channel capacity | `OBSCURA_WS_ACK_BUFFER_SIZE` | `100` | No |
| `--ack-batch-size` | WS ACK DB batch size | `OBSCURA_WS_ACK_BATCH_SIZE` | `50` | No |
| `--ack-flush-interval-ms` | WS ACK DB flush interval | `OBSCURA_WS_ACK_FLUSH_INTERVAL_MS` | `500` | No |
| `--pre-key-refill-threshold` | Threshold to trigger client refill notification | `OBSCURA_PRE_KEY_REFILL_THRESHOLD` | `20` | No |
| `--max-pre-keys` | Max One-Time PreKeys allowed per user | `OBSCURA_MAX_PRE_KEYS` | `100` | No |
| `--bucket` | S3 Bucket Name | `OBSCURA_S3_BUCKET` | - | **Yes** |
| `--region` | S3 Region | `OBSCURA_S3_REGION` | `us-east-1` | No |
| `--endpoint` | Custom S3 Endpoint (e.g., for MinIO) | `OBSCURA_S3_ENDPOINT` | - | No |
| `--access-key` | S3 Access Key ID | `OBSCURA_S3_ACCESS_KEY` | - | No |
| `--secret-key` | S3 Secret Access Key | `OBSCURA_S3_SECRET_KEY` | - | No |
| `--force-path-style` | Force Path Style (Required for MinIO) | `OBSCURA_S3_FORCE_PATH_STYLE` | `false` | No |
| `--attachment-max-size-bytes` | Max attachment size in bytes | `OBSCURA_S3_MAX_SIZE_BYTES` | `52428800` | No |

### Example

```bash
# Using Flags
./obscura-server \
  --database-url postgres://user:pass@localhost/db \
  --jwt-secret my_secret \
  --bucket my-attachments \
  --port 8080

# Using Environment Variables
export OBSCURA_DATABASE_URL=postgres://user:pass@localhost/db
export OBSCURA_JWT_SECRET=my_secret
export OBSCURA_S3_BUCKET=my-attachments
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
   docker run -d \
     -p 3000:3000 \
     -e OBSCURA_DATABASE_URL="postgres://user:pass@host.docker.internal:5432/obscura" \
     -e OBSCURA_JWT_SECRET="your_secret_key" \
     -e OBSCURA_S3_BUCKET="obscura-attachments" \
     obscura-server
   ```

### Docker Compose

A `docker-compose.yml` is provided for a complete local stack (Postgres + Server):

```bash
docker compose up -d
```

## Development

### Prerequisites
- Rust 1.83+
- PostgreSQL 16+
- `protoc` (Protocol Buffers compiler)

### Running Locally

1. Start Postgres and MinIO:
   ```bash
   docker compose up -d db minio
   ```

2. Run the server:
   ```bash
   export OBSCURA_DATABASE_URL=postgres://user:password@localhost/signal_server
   export OBSCURA_JWT_SECRET=test
   export OBSCURA_S3_BUCKET=test
   cargo run
   ```
   *Migrations are applied automatically on startup.*

### Testing

```bash
`cargo test`
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

- **REST API**: Defined in `specs/001-signal-server/contracts/openapi.yaml`.
- **WebSocket**: Available at `/v1/gateway`. Expects Protobuf messages defined in `proto/obscura.proto`.

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
