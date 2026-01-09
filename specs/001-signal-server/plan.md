# Implementation Plan: Signal Server

**Branch**: `001-signal-server` | **Date**: 2026-01-06 | **Spec**: [specs/001-signal-server/spec.md](specs/001-signal-server/spec.md)
**Input**: Feature specification from `specs/001-signal-server/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

This feature implements a minimalist, zero-knowledge Signal Protocol relay server. It enables users to register, upload cryptographic keys, and exchange end-to-end encrypted messages asynchronously. The system uses a hybrid transport model (HTTP for keys/auth, WebSocket for messaging) and enforces strict storage limits (TTL and capacity) to maintain a low footprint. Protocol Buffers are used for message serialization to ensure type safety and efficiency.

## Technical Context

**Language/Version**: Rust 1.83+
**Primary Dependencies**: `tokio` (Async Runtime), `axum` (Web Framework), `sqlx` (PostgreSQL Driver), `argon2` (Password Hashing), `serde` (Serialization), `prost` (Protobuf), `tracing` (Logging).
**Storage**: PostgreSQL
**Testing**: `cargo test` (Unit/Integration), `sqlx::test` (Database tests).
**Target Platform**: Linux server
**Project Type**: Single project (Backend API)
**Performance Goals**: Key retrieval < 200ms (P95). Efficient handling of high-churn message storage (Write/Delete).
**Constraints**: Zero-knowledge (server sees only encrypted blobs), strict message TTL, max mailbox size.
**Scale/Scope**: Minimalist MVP. Basic auth, key management, and message relay.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

This project enforces the repository Constitution (see
`.specify/memory/constitution.md`). Before Phase 0 research completes, the
plan MUST include evidence for the following gates:

- **Code Quality**:
  - Modular architecture: `api` (routes), `core` (business logic), `storage` (db).
  - Standard Rust formatting (`cargo fmt`) and linting (`cargo clippy`).
  - Documentation for all public modules.
- **Testing Standards**:
  - Unit tests for core logic (e.g., limits, validation).
  - Integration tests for the full flow: Register -> Upload Keys -> Fetch Keys -> Send Message -> Receive Message.
  - `sqlx` test helpers for database interactions.
- **Security**:
  - **Zero Knowledge**: Server treats message content as opaque blobs.
  - **Auth**: `argon2` for password hashing. Session tokens (e.g., JWT or Signed Cookies) for API access.
  - **Secrets**: DB credentials loaded from environment variables (e.g., `DATABASE_URL`).
- **Privacy**:
  - **Data Minimization**: Only store necessary metadata (sender, recipient, timestamp).
  - **Retention**: Strict TTL enforcement deletes old messages.
- **Performance**:
  - Async I/O with `tokio` for high concurrency.
  - Connection pooling with `sqlx`.
  - Database indexes on `recipient_id` for fast message retrieval.

## Project Structure

### Documentation (this feature)

```text
specs/001-signal-server/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── obscura.proto
│   └── openapi.yaml
└── tasks.md             # Phase 2 output
```

### Source Code (repository root)

```text
src/
├── main.rs              # Entry point
├── config.rs            # Configuration loader
├── error.rs             # Global error types
├── api/                 # HTTP & WebSocket handlers
│   ├── mod.rs
│   ├── auth.rs
│   ├── keys.rs
│   └── messages.rs
├── core/                # Business logic & Domain types
│   ├── mod.rs
│   ├── user.rs
│   └── crypto.rs
└── storage/             # Database layer
    ├── mod.rs
    ├── postgres.rs
    └── migrations/
```

**Structure Decision**: Standard Rust binary structure with modular separation of concerns (API, Core, Storage).

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| [e.g., 4th project] | [current need] | [why 3 projects insufficient] |
| [e.g., Repository pattern] | [specific problem] | [why direct DB access insufficient] |
