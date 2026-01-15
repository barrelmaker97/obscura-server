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
| `--database-url` | PostgreSQL connection string | `DATABASE_URL` | - | **Yes** |
| `--jwt-secret` | Secret key for signing JWTs | `JWT_SECRET` | - | **Yes** |
| `--server-host` | Interface to bind the server to | `SERVER_HOST` | `0.0.0.0` | No |
| `--port` | Port to bind the server to | `PORT` | `3000` | No |
| `--message-ttl-days` | Days before a message is auto-deleted | `MESSAGE_TTL_DAYS` | `30` | No |
| `--max-inbox-size` | Max pending messages per user | `MAX_INBOX_SIZE` | `1000` | No |
| `--rate-limit-per-second` | API requests allowed per second | `RATE_LIMIT_PER_SECOND` | `10` | No |
| `--rate-limit-burst` | Max API burst allowance per IP | `RATE_LIMIT_BURST` | `20` | No |

### Example

```bash
# Using Flags
./obscura-server \
  --database-url postgres://user:pass@localhost/db \
  --jwt-secret my_secret \
  --port 8080

# Using Environment Variables
export DATABASE_URL=postgres://user:pass@localhost/db
export JWT_SECRET=my_secret
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
     -e DATABASE_URL="postgres://user:pass@host.docker.internal:5432/obscura" \
     -e JWT_SECRET="your_secret_key" \
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

1. Start Postgres:
   ```bash
   docker compose up -d db
   ```

2. Run the server:
   ```bash
   export DATABASE_URL=postgres://user:password@localhost/signal_server
   export JWT_SECRET=test
   cargo run
   ```
   *Migrations are applied automatically on startup.*

### Testing

```bash
cargo test
```

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
