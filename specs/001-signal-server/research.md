# Research & Decisions: Signal Protocol Relay Server

**Feature**: Signal Server (`001-signal-server`)
**Date**: 2025-12-26

## Decisions

### 1. Web & WebSocket Framework: `axum`

- **Decision**: Use `axum` for both HTTP REST endpoints and WebSocket handling.
- **Rationale**: 
  - Explicitly requested by user.
  - Built on `tokio` and `hyper`, ensuring high performance.
  - Excellent extraction model for request data (Json, Query, etc.).
  - First-class WebSocket support via `axum::extract::ws`.
- **Alternatives Considered**: `actix-web` (also fast, but user requested tokio/axum stack), `warp` (filter chain can be complex).

### 2. Database Driver: `sqlx`

- **Decision**: Use `sqlx` with PostgreSQL.
- **Rationale**:
  - Fully async, works naturally with `tokio`.
  - Compile-time query verification (macros) prevents SQL syntax errors.
  - Built-in connection pooling.
  - Simple migration management via `sqlx-cli`.
- **Alternatives Considered**: `diesel-async` (ORM might be overkill for this simple schema), `tokio-postgres` (lower level, less ergonomic).

### 3. Authentication: `argon2` + JWT

- **Decision**: 
  - Use `argon2` crate for password hashing (OWASP recommended).
  - Use `jsonwebtoken` for stateless session management.
- **Rationale**: 
  - Signal clients are typically mobile/desktop apps, making stateless JWTs easier to manage than cookies.
  - JWTs allow "claims" (like User ID) to be embedded, reducing DB lookups for auth checks.
- **Alternatives Considered**: `paseto` (more secure defaults but less ubiquitously supported in client libs), Session Cookies (less ideal for non-browser clients).

### 4. WebSocket Protocol: Protocol Buffers

- **Decision**: Use Protocol Buffers (`prost`) for WebSocket messages.
- **Rationale**: 
  - **Type Safety**: Enforces a strict schema defined in `obscura.proto`.
  - **Performance**: Smaller payload size and faster serialization/deserialization compared to JSON.
  - **Consistency**: Aligns with the overall architectural choice to use structured binary formats for the secure messaging layer.
- **Protocol Draft**:
  - **Schema**: Defined in `obscura.proto`.
  - **Frame**: `WebSocketFrame` message containing `oneof` payload (Envelope, Ack, Heartbeat, Error).

### 5. WebSocket Authentication: Query Parameter

- **Decision**: Authenticate WebSocket connections via `?token=<jwt>` query parameter.
- **Rationale**:
  - Browser `WebSocket` API does not support custom headers (like `Authorization`).
  - Allows easy integration with standard web clients.
- **Security Note**: Token is visible in URL logs if not careful, but acceptable for this MVP as connection is TLS-encrypted.

### 6. Message Delivery: Fire-and-Forget

- **Decision**: Server deletes messages immediately after writing to the WebSocket buffer.
- **Rationale**:
  - Simplifies server state (no ACK tracking).
  - TCP provides transport-level reliability.
  - Application-level ACKs add significant complexity (retry logic, timeouts) out of scope for MVP.

### 7. PreKey Exhaustion: Strict Failure

- **Decision**: Return an error (e.g. 404/412) if a user has no One-Time PreKeys left.
- **Rationale**:
  - Prioritizes Forward Secrecy over Availability.
  - Forces clients to replenish keys actively.
  - Simpler than implementing fallback logic.

### 8. Serialization: `serde`

- **Decision**: Use `serde` with `serde_json`.
- **Rationale**: The de-facto standard in Rust.

## Unknowns Resolved

- **Batch Uploads**: Will be handled as a JSON array of objects in the HTTP POST body.
- **Zero Knowledge**: Verified that server only needs to store `TEXT` or `BYTEA` blobs. The "content" is encrypted by the client. The server does not need the keys.
- **Limits**: TTL 30 days, Max 1000 messages.
