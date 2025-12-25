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
- Chosen: WebSockets (Axum's WebSocket support) per connected device for real-time notifications and receipts. For the MVP, a single-server deployment is assumed. Therefore, cross-instance communication is not required. A simple, in-process message bus (e.g., `tokio::sync::broadcast`) will be used to deliver events to clients connected to the single instance.
- Rationale: This dramatically simplifies the MVP implementation by removing the need for external dependencies like Redis or the complexity of Postgres `LISTEN/NOTIFY`. It meets the immediate requirement and can be scaled out with a proper message broker in a future phase.

Decision: Authentication & Device Model
- Chosen: Username/password registration issues a short-lived access token (JWT or opaque token). Each device registers a public key. For each request that requires device authenticity, the client must provide a signature.
- **Device Signature Scheme**: The chosen scheme is a simple body-plus-timestamp signature.
    - The client must include two headers:
        1. `X-Obscura-Timestamp`: An ISO 8601 UTC timestamp of when the request was created.
        2. `X-Obscura-Signature`: The signature, calculated as `HMAC-SHA256(device_private_key, SHA256(request_body) + "|" + timestamp_string)`.
    - The server verifies that the timestamp is recent (e.g., within 30 seconds of server time) to prevent replay attacks, then re-calculates the signature to authenticate the request.
- Rationale: Matches spec clarifications for verifying device signatures. The chosen scheme is simple to implement and protects against both basic replay attacks and request body tampering.
- Alternatives: OAuth2 flows — considered but too heavyweight for MVP.

Decision: End-to-End Encryption (Cryptography)
- Chosen: Signal protocol model: X3DH for initial key agreement and Double Ratchet for message sequencing and forward secrecy. For attachments (large blobs), derive a symmetric content encryption key from the ratchet and use it to encrypt the file; then encrypt that symmetric key for the recipient's ratchet state.
- Rationale: Signal's model provides strong forward secrecy, plausible deniability, and established security properties.
- Implementation note: Use the official Signal project's Rust implementation `signalapp/libsignal` (https://github.com/signalapp/libsignal). This crate provides a maintained, first-party Rust implementation of X3DH and Double Ratchet and removes the need for FFI to C libraries for the core protocol primitives.
- Security posture: By adopting `signalapp/libsignal` we depend on the official implementation used by Signal's clients; this materially reduces the audit burden. Nonetheless, the integration surface (how keys/IDs are serialized, API handling, and any FFI boundaries if used elsewhere) still requires a brief integration review.
- Migration note: If future requirements demand different primitives or multi-device session stores, the `crypto/` integration layer should be designed so the `libsignal` usage is abstracted behind a small API boundary.

Decision: Attachment Encryption & Upload Flow
- Flow:
  1. Sender's client derives a symmetric encryption key from its Double Ratchet state.
  2. Client encrypts the photo locally using this key (e.g., AES-GCM).
  3. Client requests a presigned S3 upload URL from the server (`POST /attachments/presign`). This request is authenticated.
  4. Server returns a short-lived presigned URL.
  5. Client uploads the encrypted blob directly to S3 using the URL. The URL resolves to a unique object key (`storage_pointer`).
  6. After a successful upload, the client sends the final message by calling `POST /messages`, including the recipient's username and the `storage_pointer`.
  7. The server receives this call, creates the `message` record, and triggers a real-time push to the recipient's device.
- Rationale: This flow removes the "dangling intent" problem from the database. It can, however, create orphaned S3 objects if the client uploads but fails to send the message. This will be managed by a bucket lifecycle policy that deletes objects in the upload prefix after a short period (e.g., 24 hours).

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
