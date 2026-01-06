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

You can use `websocat` for the WebSocket and `curl` for the HTTP API.

1. **Register User A**:
   ```bash
   curl -X POST http://localhost:3000/auth/register \
     -H "Content-Type: application/json" \
     -d '{"username":"alice", "password":"password"}'
   ```

2. **Login (Get Token)**:
   ```bash
   TOKEN_A=$(curl -s -X POST http://localhost:3000/auth/login \
     -H "Content-Type: application/json" \
     -d '{"username":"alice", "password":"password"}' | jq -r .token)
   ```

3. **Connect to WebSocket**:
   ```bash
   websocat "ws://localhost:3000/ws" -H "Authorization: Bearer $TOKEN_A"
   ```

4. **Send a Message (in another terminal, as User B)**:
   (Repeat steps 1-2 for Bob to get TOKEN_B)
   ```bash
   # Connect as Bob and send JSON
   websocat "ws://localhost:3000/ws" -H "Authorization: Bearer $TOKEN_B"
   # Type: {"action": "send", "recipient_id": "<ALICE_UUID>", "ciphertext": "..."}
   ```
