# Obscura Server

Signal Protocol Relay Server.

## Prerequisites

- Rust 1.83+
- PostgreSQL 16+
- Protocol Buffers Compiler (`protoc`) - Bundled in build but requires `cmake` if building from source, or use provided setup.

## Setup

1. `docker compose up -d` (Starts PostgreSQL)
2. Configure environment variables:
   ```bash
   export DATABASE_URL=postgres://user:password@localhost/signal_server
   export JWT_SECRET=your_secret_key
   ```
   (Or pass them directly when running)
3. `cargo run` (Migrations are applied automatically).

## Testing

Tests automatically apply migrations to the database. Ensure Postgres is running.

```bash
cargo test
```

## API

See `specs/001-signal-server/contracts/openapi.yaml` for REST API.
WebSocket at `/v1/gateway`.
