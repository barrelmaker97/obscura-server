# Quickstart: Signal Protocol Relay Server

## Prerequisites

- **Rust**: 1.75+ (`rustup update stable`)
- **PostgreSQL**: 14+ running locally
- **Tools**: `sqlx-cli` (`cargo install sqlx-cli`)

## Setup

1. **Clone & Enter**:
   ```bash
   git clone <repo>
   cd obscura-server
   ```

2. **Environment**:
   Copy `.env.example` to `.env`:
   ```bash
   DATABASE_URL=postgres://user:password@localhost/signal_server
   JWT_SECRET=your_secret_key_change_me
   RUST_LOG=debug
   ```

3. **Database**:
   Create the database and run migrations:
   ```bash
   sqlx database create
   sqlx migrate run
   ```

## Running the Server

```bash
cargo run
```
Server will start on `http://127.0.0.1:3000`.

## Testing the Flow

Since the server uses **Protocol Buffers** (binary) for WebSocket communication, manual testing with `curl` or `websocat` is limited. The recommended way to verify functionality is via the included integration tests.

1. **Run Integration Tests**:
   ```bash
   cargo test --test integration_flow
   ```
   This will execute the full "Register -> Upload Keys -> Send -> Receive" cycle.

2. **Manual Connectivity Check**:
   You can still use `websocat` to verify the connection handshake, though you won't be able to send valid frames manually without a Protobuf encoder.
   ```bash
   # Connect to WebSocket (expect binary noise or immediate disconnect if idle)
   websocat "ws://localhost:3000/v1/gateway?token=$TOKEN_A"
   ```
