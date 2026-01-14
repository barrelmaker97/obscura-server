# Phase 0: Research & Technical Approach

**Feature**: Device Takeover
**Status**: Complete

## 1. API Design: Overload `POST /keys`
**Decision**: Modify the existing `POST /keys` endpoint to accept an optional `identityKey` field.
**Rationale**:
- **Atomicity**: Handles "Delete Old" and "Insert New" in a single transaction.
- **Client Simplicity**: Client simply uploads its keys; server decides if it's a refill or a takeover.
- **Safety**: Avoids "Limbo State" where a user deletes old keys but fails to upload new ones.
**Mitigation**:
- To prevent accidental nukes, the server will check if the uploaded `identityKey` matches the existing one.
    - If match: Treat as standard pre-key refill (do nothing destructive).
    - If mismatch: Trigger full "Device Takeover" (wipe old data).
**Alternatives Considered**:
- *New Endpoint (`POST /device/reset`)*: Rejected due to risk of state drift and "Gap of Death" (client state inconsistent with server).

## 2. Notification System Refactor
**Decision**: Refactor `InMemoryNotifier` to broadcast a typed Enum `UserEvent` instead of `()`.
```rust
pub enum UserEvent {
    MessageReceived,
    Disconnect,
}
```
**Rationale**:
- The current implementation only signals "something happened".
- `gateway.rs` needs to distinguish between "fetch new messages" and "terminate connection immediately".
**Alternatives Considered**:
- *Separate "Control" Channel*: Adding a second `DashMap` for control signals adds complexity and synchronization overhead.

## 3. WebSocket "Key-Gating"
**Decision**: Add a check in `websocket_handler` (or `handle_socket`) to query `KeyRepository` for an existing Identity Key before entering the main loop.
**Rationale**:
- Enforces FR-008 ("Reject connections without identity key").
- Prevents "ghost" users (registered but keyless) from consuming socket resources.

## 4. Data Cleanup Implementation
**Decision**: Implement explicit `delete_all_pending_messages(user_id)` in `MessageRepository` and `delete_all_pre_keys(user_id)` in `KeyRepository`.
**Rationale**:
- Privacy requirement to remove undecryptable messages.
- Protocol requirement to invalidate old pre-keys signed by the old Identity Key.
