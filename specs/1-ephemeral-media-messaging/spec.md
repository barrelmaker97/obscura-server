# Feature: Ephemeral Media Messaging (Backend)

## Summary
Build a privacy-first, media-centric messaging backend that enables 1-on-1 end-to-end encrypted (E2EE) photo sharing between users discovered by unique usernames. Photos are delivered in real time, display read receipts, and "burn" (are irreversibly deleted from all servers and devices) immediately after viewing. The server acts as a blind courier and cannot decrypt message content.

## Background and Goals
Users want the spontaneity and ephemeral nature of early social apps while keeping strong privacy and minimal metadata exposure. This backend MVP focuses on secure 1-on-1 photo messages with real-time delivery and read receipts, supporting high-performance delivery for mobile clients while following zero-knowledge principles.

## Actors
- **User**: A human with a unique username and one or more devices registered to their account.
- **Device**: A client instance (mobile or desktop) that holds user keys and receives messages.
- **Server**: The backend message relay and storage (blind courier; stores only encrypted blobs and limited metadata required for routing).
- **Admin (operator)**: Maintains infrastructure; has no access to plaintext content.

## User Scenarios & Testing
- **S1 — Find and add friend**: Alice searches for `bob42` and sends a friend request or directly sends a message if discovery is allowed. Test: Searching `bob42` returns the correct user; sending invitation or message flows succeed end-to-end.
- **S2 — Send photo**: Alice captures a photo, client encrypts it end-to-end to Bob, and uploads the encrypted payload to the server. Server stores only encrypted blob and delivery metadata. Test: Server receives encrypted blob; recipient receives encrypted blob; server logs show no plaintext.
- **S3 — Real-time delivery and receipt**: When Bob is online, the message is delivered in under 2 seconds and a "delivered" indicator is shown to Alice. When Bob opens the photo, a "read" receipt is returned to Alice and the photo burns everywhere. Test: Measure delivery latency, and verify read receipt is received and post-view content is deleted.
- **S4 — Offline recipient**: Alice sends to offline Bob. Server persists encrypted blob for up to 7 days; upon Bob's next device connection, the message delivers and then burns after view. Test: Send while offline, then simulate Bob connecting after N hours and verify delivery and burn.
- **S5 — Multiple devices**: Bob has two devices. Message should deliver to all registered devices; once the image is viewed on any device it must be removed from all devices and from server. Test: Deliver to device A and B, open on A, verify deletion on B and server.

## Functional Requirements (testable)
1. **Unique Usernames**: Users register a unique `username` (3–30 characters, alphanumeric + underscores). Tests: Registration rejects duplicates and invalid formats.
2. **Account & Device Keys**: Each device holds private keys; server stores only device public keys for routing. Tests: Server endpoints accept device public keys but cannot decrypt message payloads.
3. **End-to-end Encryption (E2EE)**: Photo payloads are encrypted by sender client for recipient device keys; server stores only ciphertext. Tests: All stored blobs are inscrutable when decrypted attempt is made server-side.
4. **Burn-on-View**: When a recipient views the photo, the system must ensure the photo is irreversibly deleted from server storage and instructs all recipient devices to securely delete local copies. Tests: After view, server contains no retrievable copy and devices remove local copies.
5. **Real-time Delivery**: Use push or persistent connection to deliver messages in <2s for online users under nominal load. Tests: Delivery latency measured under baseline load.
6. **Read & Delivery Receipts**: Provide delivered and read receipts to sender; receipts do not leak photo content. Tests: Receipts are delivered within specified time bounds and are associated only with message IDs.
7. **Offline Delivery Window**: Undelivered messages retained for a configurable default of 7 days; after that, messages are purged. Tests: Message removed after retention window when still unclaimed.
8. **Zero-Knowledge Metadata Minimization**: Server stores minimal routing metadata (sender id, recipient id, ciphertext id, and timestamps); no plaintext or derived thumbnails retained. Tests: Audit of stored fields shows no content or thumbnails.
9. **Anonymity & Presence Controls**: Users may opt out of online presence broadcasting; read receipts remain functional. Tests: Presence opt-out hides online status but does not block receipt functionality.
10. **Rate Limits & Abuse Mitigation**: Throttle new-username lookups and message uploads per IP/account to mitigate scraping/abuse. Tests: Rate limits enforced under simulated attack.
11. **Auditability**: Operators can verify delivery status and retention without accessing content. Tests: Operator logs and APIs provide delivery metadata but not content.

## Success Criteria (measurable, tech-agnostic)
- **Delivery latency**: 95% of messages to online users deliver within 2 seconds under baseline load.
- **Retention enforcement**: 100% of unviewed messages are purged within 24 hours after the 7-day retention window expires.
- **Burn reliability**: 100% of viewed messages are removed from server storage within 5 seconds of view acknowledgement, and devices are instructed to delete local copies.
- **Zero-knowledge**: Independent audit (black-box) cannot retrieve any plaintext content from stored server artifacts.
- **Scalability baseline**: MVP supports 10,000 concurrent connected devices with sustained message throughput consistent with low-latency delivery above.
- **Usability**: New user can find and send a message to another user in under 90 seconds from registration.

## Key Entities
- `User` (id, username, public profile metadata minimal)
- `Device` (id, user_id, device_public_key, device_label)
- `PhotoMessage` (id, sender_id, recipient_id, ciphertext_blob_id, created_at, expires_at, retention_deadline)
- `DeliveryReceipt` (message_id, device_id, status: queued/delivered/read, timestamp)
- `CiphertextBlob` (id, storage_pointer, size_bytes, checksum)

## Assumptions
- Clients implement secure key generation and storage (private keys never leave device).
- The client will handle local secure deletion of images upon burn instruction.
- Default offline retention is 7 days; operators can configure shorter windows per region.
- Discovery is username-based; there is no global phonebook import in MVP.

## Non-Goals
- Group messaging is out of scope for the MVP.
- Rich media transforms (thumbnails or server-side resizing) are out of scope to preserve zero-knowledge.
- Cross-posting to external services is not supported.

## Acceptance Criteria / Tests (high level)
- AC1: Register two users with unique usernames; send an encrypted photo from A→B; B receives ciphertext only; after view by B, sender receives read receipt and server shows no stored blob for that message.
- AC2: Send to offline recipient; the ciphertext is stored and delivered on next device connect within retention window; after view, the ciphertext is removed.
- AC3: Multiple devices: message delivered to all, view on any device triggers deletion on the others and on server.
- AC4: Attempt to request message content via operator interfaces or logs returns no plaintext and only metadata.

## Files created by this spec
- `specs/1-ephemeral-media-messaging/spec.md`
- `specs/1-ephemeral-media-messaging/checklists/requirements.md`

## Notes
- Document contains no implementation language, only behaviour and measurable outcomes.
- Security-sensitive items should be validated with a cryptographic design review before implementation.

--
Generated: 2025-12-17
