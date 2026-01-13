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

2. **Database**:
   Start PostgreSQL using Docker Compose:
   ```bash
   docker compose up -d
   ```

3. **Environment**:
   Set the required environment variables:
   ```bash
   export DATABASE_URL=postgres://user:password@localhost/signal_server
   export JWT_SECRET=test_secret
   ```
   ```bash
   DATABASE_URL=postgres://user:password@localhost/signal_server
   JWT_SECRET=your_secret_key_change_me
   RUST_LOG=debug
   ```

## Running the Server

```bash
cargo run
```
Server will start on `http://0.0.0.0:3000`.
**Note**: Migrations are applied automatically on startup and during tests.

## Testing the Flow

1. **Automated Tests**:
   Run the full integration suite:
   ```bash
   cargo test
   ```
   Tests will execute all integration flows and automatically ensure the database schema is up-to-date.

2. **Manual Connectivity Check**:
   You can use `websocat` to verify the connection handshake. Note that you need a valid JWT token from the registration/login flow.
   You won't be able to send valid frames manually without a Protobuf encoder.
   ```bash
   # Connect to WebSocket (expect binary noise or immediate disconnect if idle)
   websocat "ws://localhost:3000/v1/gateway?token=$TOKEN"
   ```
