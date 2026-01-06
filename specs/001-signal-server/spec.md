# Feature Specification: Signal Protocol Relay Server

**Feature Branch**: `001-signal-server`
**Created**: 2025-12-26
**Status**: Draft
**Input**: User description: "Build an implementation of a server for the signal protocol, covering the basic features of sending and receiving messages. It is a minimalist, zero-knowledge relay to allow async E2EE messaging between users."

## Clarifications

### Session 2025-12-26
- Q: How are User IDs generated and users authenticated? → A: Users identify via a chosen **Username** and authenticate with a **Password**.
- Q: How to handle offline storage limits? → A: Enforce **both** a max message count (FIFO drop) AND a strict Time-To-Live (TTL) for all messages.
- Q: How are One-Time PreKeys uploaded? → A: **Grouped into batches** (e.g., 50 at a time).
- Q: Transport protocol strategy? → A: **Hybrid**: HTTP for Users/Keys, WebSocket for Message Relay.

### Session 2026-01-05
- Q: WebSocket Message Reliability? → A: **Fire-and-Forget**. Server deletes message immediately after writing to the socket buffer. No application-level ACK required.
- Q: WebSocket Authentication? → A: **Query Parameter**. Usage: `ws://host/ws?token=<jwt>`.
- Q: Specific Storage Limits? → A: **TTL**: 30 Days. **Capacity**: 1000 messages per user (FIFO).
- Q: PreKey Exhaustion behavior? → A: **Strict Failure**. If no One-Time PreKeys are available, the server returns an error (no fallback to just Signed PreKey).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - User Registration & Key Publishing (Priority: P1)

Users must be able to register with the server to establish an inbox and publish the cryptographic keys necessary for other users to initiate encrypted sessions with them.

**Why this priority**: Without public keys stored on the server (PreKeys), the "asynchronous" part of the Signal Protocol is impossible; users would need to be online simultaneously to handshake.

**Independent Test**: A client can upload their Identity Key, Signed PreKey, and a batch of One-Time PreKeys. A second, different client can successfully retrieve these keys by querying the first user's ID.

**Acceptance Scenarios**:

1. **Given** a new user with a generated key pair, **When** they POST their Username, Password, Identity Key, and PreKeys (in a batch) to the registration endpoint, **Then** the server responds with success (200 OK) and stores the keys.
2. **Given** a registered user, **When** another user requests their PreKey bundle, **Then** the server returns the Identity Key, Signed PreKey, and one One-Time PreKey (removing it from storage).
3. **Given** a registration request, **When** the payload is missing required keys, **Then** the server returns a validation error (400 Bad Request).
4. **Given** a user with 0 remaining One-Time PreKeys, **When** another user requests a PreKey bundle, **Then** the server returns an error (404 Not Found or 412 Precondition Failed), preventing the handshake.

---

### User Story 2 - Asynchronous Message Exchange (Priority: P1)

Users must be able to send encrypted messages to offline users, which the server holds until the recipient retrieves them.

**Why this priority**: This is the core "relay" functionality.

**Independent Test**: Client A sends a message to Client B. Client B (simulating being offline initially) then requests messages and receives the payload. The payload is verified to be removed from the server after fetch.

**Acceptance Scenarios**:

1. **Given** a registered User B, **When** User A sends an encrypted message payload addressed to User B via WebSocket, **Then** the server accepts it and stores it in User B's inbox.
2. **Given** User B has pending messages, **When** User B connects and requests their messages, **Then** the server pushes the encrypted payloads.
3. **Given** User B has retrieved their messages, **When** they request messages again, **Then** the inbox is empty (messages are deleted immediately upon push).

---

### Edge Cases

- What happens when a user runs out of One-Time PreKeys? (Strict Failure: see FR-003).
- How does the system handle message storage limits for a user who never comes online? (See FR-009: 30-day TTL, 1000 msg cap)
- What happens if two users try to register the same User ID?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST allow users to register an account identified by a unique **Username** and authenticate via **Password** (securely hashed) over HTTP.
- **FR-002**: System MUST allow users to upload and update their "PreKey Bundles" (Identity Key, Signed PreKey, and batches of One-Time PreKeys) via HTTP.
- **FR-003**: System MUST provide an HTTP endpoint to retrieve a targeted user's PreKey Bundle. It MUST consume one One-Time PreKey per request and fail with an error if none are available.
- **FR-004**: System MUST allow authenticated users to send encrypted binary payloads to another user's inbox via WebSocket.
- **FR-005**: System MUST allow authenticated users to list and fetch encrypted messages from their own inbox via WebSocket.
- **FR-006**: System MUST delete messages from storage immediately after they are written to the WebSocket (Fire-and-Forget).
- **FR-007**: System MUST use a **Hybrid Transport** model: HTTP for user/key management, and WebSocket for real-time message sending/receiving.
- **FR-008**: System MUST NOT store any message content in plaintext or retain keys that would allow decryption (Zero-Knowledge).
- **FR-009**: System MUST enforce storage limits: **30-Day TTL** for all messages AND a **maximum of 1000 messages** per inbox (oldest messages dropped first if full).
- **FR-010**: WebSocket connections MUST be authenticated via a **Query Parameter** (e.g., `?token=...`).

### Constitution Alignment (mandatory)

- **Code Quality**: Code must be modular, separating the "Key Store" logic from the "Message Relay" logic.
- **Testing Standards**: Integration tests must verify the full "upload keys -> fetch keys -> send message -> fetch message" cycle.
- **Security**: 
    - No plaintext storage of messages.
    - Authentication via Password (bcrypt/argon2 hashing required) and standard session management (e.g., JWT).
    - Rate limiting on API endpoints to prevent DOS.
- **User Privacy**: Server stores metadata (sender/receiver IDs, timestamps) but zero content. Logs must not contain message payloads or key material.
- **Performance & Reliability**: Database interactions for message storage should be optimized for high write/delete churn.

### Key Entities

- **User**: Represents an identity (Username, PasswordHash).
- **PreKeyBundle**: Collection of public keys (Identity, Signed, One-Time) stored for a User.
- **Message**: Encrypted blob, Timestamp, SenderID, stored in a User's Inbox.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A complete "Hello World" exchange (Registration A & B -> A gets B's keys -> A sends -> B receives) completes successfully in tests.
- **SC-002**: Server storage footprint for messages drops to near zero when all recipients have synced (verifying deletion).
- **SC-003**: API response time for key retrieval is under 200ms (P95) to ensure fast session setup.
