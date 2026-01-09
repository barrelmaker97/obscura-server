# Quickstart: Signal Protocol Relay Server

## Prerequisites

- **Rust**: 1.83+ (`rustup update stable`)
- **PostgreSQL**: 16+ running locally (or via Docker)
- **Protobuf Compiler**: `protoc` (Required for build)
  - Ubuntu/Debian: `sudo apt install protobuf-compiler`
  - MacOS: `brew install protobuf`

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
   You can use Docker to spin up a quick database:
   ```bash
   docker run --name obscura-db -e POSTGRES_USER=user -e POSTGRES_PASSWORD=password -e POSTGRES_DB=signal_server -p 5432:5432 -d postgres:16
   ```

## Running the Server

```bash
cargo run
```
Server will start on `http://0.0.0.0:3000`.
**Note**: Migrations are applied automatically on startup.

## Testing the Flow

Since the server uses **Protocol Buffers** (binary) for WebSocket communication, manual testing with `curl` or `websocat` is limited. The recommended way to verify functionality is via the included integration tests.

1. **Run Integration Tests**:
   ```bash
   cargo test
   ```
   This will execute all tests, including:
   - `integration_registration`: Register -> Upload Keys -> Fetch Keys
   - `integration_messaging`: Send Message -> Receive via WebSocket

2. **Manual Connectivity Check**:
   You can still use `websocat` to verify the connection handshake, though you won't be able to send valid frames manually without a Protobuf encoder.
   ```bash
   # Connect to WebSocket (expect binary noise or immediate disconnect if idle)
   websocat "ws://localhost:3000/v1/gateway?token=$TOKEN_A"
   ```