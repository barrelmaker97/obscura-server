# Feature: Ephemeral Media Messaging (Backend)

## Summary
Build a privacy-first, media-centric messaging backend that enables 1-on-1 end-to-end encrypted (E2EE) photo sharing between users (one active device per user). Photos are delivered in real time, display read receipts, and "burn" (are irreversibly deleted from server and recipient's device) immediately after viewing. The server acts as a blind courier and cannot decrypt message content.

## Background and Goals
Users want the spontaneity and ephemeral nature of early social apps while keeping strong privacy and minimal metadata exposure. This backend MVP focuses on secure 1-on-1 photo messages with real-time delivery and read receipts, supporting high-performance delivery for mobile clients while following zero-knowledge principles.

## Clarifications

### Session 2025-12-17
- Q: How does a user authenticate to the server? → A: Username/password (Signal-style) generates temporary access tokens. The user's single device signs requests with its private key; server verifies signature against the registered device public key before routing messages.
- Q: When a user has multiple registered devices, how should the server route messages to them? → A: MVP constraint: one device per user only. Device registration/swap requires re-authentication. Previous undelivered messages to old device are abandoned. Multi-device support deferred to post-MVP.

- Q: Should the server provide friend discovery (username search) or is there a privacy-first alternative? → A: No server-side friend search. Client requires exact username at send time. Client app maintains local contact history after first message with a user. Server has no searchable username index.
- Q: What audit logging and compliance framework should operators maintain? → A: Audit-free MVP: server logs only errors. No delivery/burn history retained. No metadata persistence. Compliance frameworks deferred to post-MVP if needed.
- Q: When should media be deleted from the server? → A: Burn immediately on view (atomic with read receipt). Unviewed messages auto-deleted 30 days after creation. No intermediate offline retention window; offline recipients must reconnect within 30 days or message is lost.

## Actors
- **User**: A human with a unique username and exactly one active device registered to their account.
- **Device**: A client instance (mobile or desktop) that holds the user's keys and receives messages. Each user has one device in MVP.
- **Server**: The backend message relay and storage (blind courier; stores only encrypted blobs and limited metadata required for routing).
- **Admin (operator)**: Maintains infrastructure; has no access to plaintext content.

## User Scenarios & Testing
- **S1 — Find and send message**: Alice knows Bob's exact username `bob42` (shared via out-of-band channel). Alice sends a photo directly to `bob42`. Server routes to Bob's single device. Test: Message successfully delivered to Bob's device; server does not provide username search capability.
- **S2 — Send photo**: Alice captures a photo, client encrypts it end-to-end to Bob, and uploads the encrypted payload to the server. Server stores only encrypted blob and delivery metadata. Test: Server receives encrypted blob; recipient receives encrypted blob; server logs show no plaintext.
- **S3 — Real-time delivery and receipt**: When Bob is online, the message is delivered to his device in under 2 seconds and a "delivered" indicator is shown to Alice. When Bob opens the photo on his device, a "read" receipt is returned to Alice and the photo is burned (deleted from server and Bob's device). Test: Measure delivery latency; verify read receipt is received and server/device copies are deleted.
- **S4 — Offline recipient & expiry**: Alice sends to offline Bob. Server persists encrypted blob; upon Bob's device reconnection at any point within 30 days, the message delivers and burns immediately after view. If Bob's device never connects, the message is auto-deleted after 30 days. Test: (A) Send while Bob offline, Bob reconnects within 30 days, verify delivery and burn on view. (B) Send to offline Bob who never reconnects within 30 days; verify deletion after 30 days.
## Functional Requirements (testable)
1. **Unique Usernames**: Users register a unique `username` (3–30 characters, alphanumeric + underscores). Tests: Registration rejects duplicates and invalid formats.
2. **Account & Device Keys**: Each user has one active device that holds private keys. Server stores only the device's public key for that user. Tests: Server endpoints accept the device public key on registration; server cannot decrypt message payloads.
3. **End-to-end Encryption (E2EE)**: Photo payloads are encrypted by sender's device for the recipient's device public key; server stores only ciphertext. Tests: All stored blobs are inscrutable when decryption is attempted server-side.
4. **Burn-on-View**: When a recipient views the photo, the system must immediately and irreversibly delete the photo from server storage and instruct the recipient's device to securely delete its local copy. Server-side deletion is atomic with the read receipt. Tests: After view, server contains no retrievable copy within 1 second; recipient's device removes local copy; attempting to fetch the deleted ciphertext returns 404.
5. **Real-time Delivery**: Use push or persistent connection to deliver messages in <2s for online users under nominal load. Tests: Delivery latency measured under baseline load.
6. **Read & Delivery Receipts**: Provide delivered and read receipts to sender; receipts do not leak photo content. Tests: Receipts are delivered within specified time bounds and are associated only with message IDs.
7. **Message Expiry**: Messages are deleted from server under two conditions: (1) Immediately upon view (burn-on-read), or (2) After 30 days of creation if never viewed (hard expiry). No intermediate retention window. Tests: Viewed message deleted within 1 second; unviewed message persisted up to 30 days then deleted; deletion is permanent and immediate.
8. **Zero-Knowledge Metadata Minimization**: Server stores minimal routing metadata (sender id, recipient id, ciphertext id, and timestamps); no plaintext or derived thumbnails retained. Tests: Audit of stored fields shows no content or thumbnails.
9. **Presence & Read Receipts**: Server provides online/offline presence for routing; read receipts are always enabled. Tests: Online presence used for <2s delivery routing; read receipts always returned on message open.
10. **Rate Limits & Abuse Mitigation**: Throttle message uploads per IP/account to mitigate abuse (no server-side search exists to throttle). Tests: Message upload rate limits enforced under simulated attack.
11. **Audit-Free Operation**: Server logs errors only; no delivery, burn, or metadata history is retained. Operators cannot query delivery status, device registration, or user activity post-facto. Tests: Verify server contains no audit logs after message burn; error logs contain no message metadata or user identifiers.

## Success Criteria (measurable, tech-agnostic)
- **Delivery latency**: 95% of messages to online users deliver within 2 seconds under baseline load.
- **Burn reliability**: 100% of viewed messages are removed from server storage within 1 second of view acknowledgement (atomic with read receipt); devices are instructed to delete local copies.
- **Expiry enforcement**: 100% of unviewed messages are purged within 24 hours after the 30-day creation timestamp expires.
- **Zero-knowledge**: Independent audit (black-box) cannot retrieve any plaintext content from stored server artifacts.
- **Scalability baseline**: MVP supports 10,000 concurrent connected users (one device per user) with sustained message throughput consistent with low-latency delivery above.
- **Usability**: New user can register, add a known contact (via exact username), and send a message in under 90 seconds.
- **Audit-free**: No delivery history, metadata logs, or audit trails are retained on the server post-burn.

## Key Entities
- `User` (id, username, device_public_key)
- `PhotoMessage` (id, sender_id, recipient_id, ciphertext_blob_id, created_at, expires_at, retention_deadline)
- `DeliveryReceipt` (message_id, status: queued/delivered/read, timestamp)
- `CiphertextBlob` (id, storage_pointer, size_bytes, checksum)

## Assumptions
- One device per user in MVP. Device registration/swap requires re-authentication. Previous messages to old device are lost.
- Clients implement secure key generation and storage (private keys never leave device); architecture follows Signal's proven E2EE model.
- The client will handle local secure deletion of images upon burn instruction.
- Message lifecycle: burned immediately on view, or auto-deleted 30 days after creation (whichever comes first). No intermediate offline retention window.
- Discovery is out-of-band: client requires exact username; no server-side search or username index exists. Client maintains local contact history.
- Authentication uses username/password with temporary server-issued tokens; device authentication is signature-based (private key signing).
- Server is audit-free MVP: no delivery history, metadata logs, user activity, or device registration history retained post-burn.

## Non-Goals
- Group messaging is out of scope for the MVP.
- Rich media transforms (thumbnails or server-side resizing) are out of scope to preserve zero-knowledge.
- Cross-posting to external services is not supported.
- Server-side friend discovery / username search is not supported (privacy-first design; client manages contacts locally).
- Multi-device support is out of scope for MVP (one device per user; device changes require re-auth).

## Acceptance Criteria / Tests (high level)
- AC1: Register two users with unique usernames; send an encrypted photo from A→B; B receives ciphertext only; after view by B, sender receives read receipt and server shows no stored blob for that message.
- AC2: Send to offline recipient; the ciphertext is stored and delivered when recipient's device reconnects (within 30 days); after view, the ciphertext is removed from server and device.
- AC3: Device change: register device A and send message; unregister device A and register device B; verify new message does not deliver to old device A.
- AC4: Attempt to request message content via operator interfaces or logs returns no plaintext, device identifiers, or user-identifying metadata.

## Files created by this spec
- `specs/1-ephemeral-media-messaging/spec.md`
- `specs/1-ephemeral-media-messaging/checklists/requirements.md`

## Notes
- Document contains no implementation language, only behaviour and measurable outcomes.
- Security-sensitive items should be validated with a cryptographic design review before implementation.

--
Generated: 2025-12-17
