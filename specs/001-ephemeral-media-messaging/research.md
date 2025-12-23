# Research: Ephemeral Media Messaging - Design Decisions

This document resolves open clarifications from the implementation plan and records design choices, rationale, and alternatives considered.

---

Decision: Language + Frameworks
- Chosen: Rust (stable toolchain) with `tokio` runtime and `axum` for HTTP + WebSocket endpoints.
- Rationale: Rust gives strong safety guarantees and high performance for a low-overhead messaging relay. `tokio` + `axum` are mature, async-first, and have good ecosystem support (WebSocket, middleware, tracing).
- Alternatives: Go (fast development and simple concurrency) — rejected because we prefer Rust's memory-safety and ecosystem alignment with the team.

Decision: Database and ORM
- Chosen: PostgreSQL with `sqlx` for async queries and compile-time checked SQL (or `sea-orm` if prefer an ORM abstraction; `sqlx` recommended for explicit SQL and lower abstraction leakage).
- Rationale: Postgres provides reliable ACID semantics, TTL-based cleanups via background jobs, and indexing required for routing metadata. `sqlx` allows writing raw SQL with compile-time guarantees.
- Alternatives: Using an ORM like `sea-orm` (easier modeling) — considered but `sqlx` preferred for explicit migrations and performance.

Decision: Object Storage
- Chosen: S3-compatible object storage accessed via `aws-sdk-s3` (endpoint override supported for S3-compatible stores). The server will generate presigned upload URLs for clients to upload encrypted blobs directly.
- Rationale: Offloads large binary transfer to object storage; presigned URLs keep server stateless for uploads and avoid buffering large files in the app server.
- Alternatives: Server-mediated uploads (proxying through backend) — rejected because it increases server bandwidth and memory.

Decision: Real-time Delivery
- Chosen: WebSockets (Axum's WebSocket support) per connected device for real-time notifications and receipts. For cross-instance delivery, use a message broker or Postgres pub/sub (Phase 2: Redis streams or Kafka recommended for higher scale). For MVP, Postgres NOTIFY / LISTEN or a simple in-memory broker with sticky sessions can be used.
- Rationale: WebSockets provide low-latency, bidirectional comms suitable for delivery/receipt events.

Decision: Authentication & Device Model
- Chosen: Username/password registration issues a short-lived access token (JWT or opaque token) to the device. Each device registers a public key (device_public_key) on account creation. For each request that requires device authenticity (e.g., message registration, presign request), the client attaches a signature created with the device private key. Server verifies signature against stored device_public_key for that user.
- Rationale: Matches spec clarifications: server verifies device signatures for critical operations.
- Alternatives: OAuth2 flows — considered but too heavyweight for MVP.

Decision: End-to-End Encryption (Cryptography)
- Chosen: Signal protocol model: X3DH for initial key agreement and Double Ratchet for message sequencing and forward secrecy. For attachments (large blobs), derive a symmetric content encryption key from the ratchet and use it to encrypt the file; then encrypt that symmetric key for the recipient's ratchet state.
- Rationale: Signal's model provides strong forward secrecy, plausible deniability, and established security properties.
- Implementation note: Use the official Signal project's Rust implementation `signalapp/libsignal` (https://github.com/signalapp/libsignal). This crate provides a maintained, first-party Rust implementation of X3DH and Double Ratchet and removes the need for FFI to C libraries for the core protocol primitives.
- Security posture: By adopting `signalapp/libsignal` we depend on the official implementation used by Signal's clients; this materially reduces the audit burden. Nonetheless, the integration surface (how keys/IDs are serialized, API handling, and any FFI boundaries if used elsewhere) still requires a brief integration review.
- Migration note: If future requirements demand different primitives or multi-device session stores, the `crypto/` integration layer should be designed so the `libsignal` usage is abstracted behind a small API boundary.

Decision: Attachment Encryption & Upload Flow
- Flow:
  1. Sender derives a compact symmetric file key from the sender's Double Ratchet state for this message.
  2. Sender uses the symmetric key to encrypt the file client-side (AES-GCM or XChaCha20-Poly1305).
  3. Sender requests a presigned S3 upload URL from server, including an authenticated, signed request proving ownership.
  4. Server verifies the request signature (device signature) and returns a presigned URL with a short TTL, storing only an expected storage pointer and metadata (size, checksum, message_id) in Postgres.
  5. Client uploads ciphertext directly to S3 using the presigned URL.
  6. The server publishes delivery metadata to recipient's connected device (or queues for offline delivery).
- Rationale: Keeps encryption client-side, S3 handles binary storage, and server only sees ciphertext.

Decision: Message Lifecycle & Burn
- The server exposes a `mark_read` or implicit read event via the WebSocket open/view event from recipient device; when the server receives a verified read event:
  - It atomically: (1) marks message status as `read`, (2) issues deletion of the stored S3 object (or marks the object deleted and calls S3 delete), and (3) emits a read receipt to the sender.
- Atomicity and verification: Server will verify the read event is signed by recipient device to mitigate fake receipts. Deletion of S3 object is best-effort: S3 delete is issued synchronously; the DB state change and delete should be performed in a two-phase approach with idempotency and retries.

Decision: Background Expiry
- Implement a scheduled background task (Tokio task or cron job) that periodically (e.g., hourly) finds messages older than 30 days that are not `read` and deletes their S3 object and DB row. Use paged deletes to avoid long-running transactions.

Decision: Cross-instance Notification
- For MVP: start with Postgres `LISTEN/NOTIFY` for cross-instance delivery signals or a simple Redis pub/sub if available. Document migration to Redis or Kafka for higher scale.

Decision: Secrets & Operations
- Use external secret manager (e.g., HashiCorp Vault, AWS Secrets Manager) for signing keys and server-side secrets (JWT signing keys). CI and deploy pipelines must not write secrets to repo.

Decision: Testing & CI
- Unit tests for serialization, DB helpers, and small crypto helpers (where safe). Integration tests using ephemeral Postgres (testcontainers) that exercise REST/WebSocket flows and S3 (use localstack or a minibucket for S3-compatible testing). Add contract tests from OpenAPI in Phase 1.

---

Alternatives Considered (short):
- Proxy uploads through server: easier to control but increases bandwidth and memory usage — rejected.
- Using a managed Signal service (not applicable) — we must implement server behavior.
- Multi-device support for MVP — deferred (increases complexity of session routing and key management).

Open Questions / NEEDS CLARIFICATION (resolved here):
- Cryptography implementation: use audited crates or FFI to Signal libs. Resolved: Use audited implementations; if none exist, use FFI to `libsignal` with mandatory security review. (Documented above.)
- Cross-instance notifications: use Postgres `NOTIFY` for MVP, migrate to Redis/Kafka later. (Decision made.)

Done: All previous `NEEDS CLARIFICATION` items resolved or deferred to Phase 2 with mitigation notes.
