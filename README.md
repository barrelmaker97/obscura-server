# Obscura Server

Signal Protocol Relay Server.

## Prerequisites

- Rust 1.83+
- PostgreSQL 16+
- Protocol Buffers Compiler (`protoc`) - Bundled in build but requires `cmake` if building from source, or use provided setup.

## Setup

1. Copy `.env.example` to `.env` and configure `DATABASE_URL` and `JWT_SECRET`.
2. `cargo run` (This will run migrations automatically).

## Testing

```bash
cargo test
```

## API

See `specs/001-signal-server/contracts/openapi.yaml` for REST API.
WebSocket at `/v1/gateway`.
