# Implementation Plan: Ephemeral Media Messaging (Backend)

**Branch**: `001-ephemeral-media-messaging` | **Date**: 2025-12-21 | **Spec**: `specs/001-ephemeral-media-messaging/spec.md`
**Input**: Feature specification from `/specs/001-ephemeral-media-messaging/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

Implement an Ephemeral Media Messaging backend that acts as a blind courier for end-to-end encrypted photo messages. The service is implemented in Rust using async foundations (Tokio) and an HTTP/WebSocket framework (Axum). The server persists only encrypted ciphertext blobs in S3-compatible object storage and minimal routing metadata in PostgreSQL. For the single-instance MVP, real-time delivery and receipts are provided via WebSockets backed by a simple in-process message bus. Presigned S3 URLs are used for direct encrypted media uploads from clients.

## Technical Context

<!--
  ACTION REQUIRED: Replace the content in this section with the technical details
  for the project. The structure here is presented in advisory capacity to guide
  the iteration process.
-->

**Language/Version**: Rust (stable toolchain; minimum Rust 1.70; target latest stable)  
**Primary Dependencies**: `tokio` (async runtime), `axum` (HTTP + WebSocket endpoints), `sqlx` (Postgres async DB access with compile-time checks), `aws-sdk-s3` (S3-compatible object storage with endpoint override) or `rusoto` alternative if needed, `serde`/`serde_json` for serialization, `tracing` for structured logging. Cryptography: use vetted Signal-like libraries or explicit integration notes (X3DH + Double Ratchet primitives implemented in audited crates; more in `research.md`).  
**Storage**: PostgreSQL for routing metadata and indices (`users`, `devices`, `messages`, `receipts`); S3-compatible object storage for encrypted ciphertext blobs (presigned upload URLs from server).  
**Real-time Delivery**: For the single-instance MVP, notifications (new messages, receipts) will be pushed to clients over WebSockets. A simple in-process fan-out message bus (e.g., `tokio::sync::broadcast`) will be used to distribute events to connected clients. This avoids the need for external dependencies like Redis or complex database features for the initial implementation.
**Testing**: `cargo test` for unit tests; `tokio::test` for async tests; integration tests under `tests/` that exercise REST and WebSocket flows; contract tests to validate OpenAPI endpoints. Use testcontainers / ephemeral Postgres instances for integration tests where applicable.  
**Target Platform**: Linux server (x86_64/amd64 or ARM64 for cloud deployment).  
**Project Type**: Backend service (single Rust crate or workspace with `server/` crate).  
**Performance Goals**: 95% of online deliveries within 2s under baseline load; support 10k concurrent connected users (one device each) in MVP; message burn operation observable within 1s of view.  
**Constraints**: Zero-knowledge server (no plaintext stored or derivable), one device per user in MVP, secrets must be stored in an external secret manager, no server-side media transforms or thumbnails, message retention: immediate burn on view or automatic deletion after 30 days.  
**Scale/Scope**: MVP targets 10k concurrent users on a single server instance. Design components should be horizontally scalable where possible (e.g. stateless request handlers) to facilitate future scaling.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

This project enforces the repository Constitution (see
`.specify/memory/constitution.md`). Before Phase 0 research completes, the
plan MUST include evidence for the following gates (one-line confirmations are
acceptable with links to details):

- Code Quality: `tracing` + `clippy` + `rustfmt` will be enforced; public API contracts will be captured in `/specs/001-ephemeral-media-messaging/contracts/openapi.yaml` (created in Phase 1). See Feature Spec sections `Functional Requirements` and `Acceptance Criteria` for required behavior.  
- Testing Standards: Unit tests via `cargo test`, async integration tests under `tests/`, and contract tests against the OpenAPI contract. Failing tests will live in `tests/` and `src/` modules alongside code; critical modules (auth, message lifecycle, storage) will have integration tests. See `Functional Requirements` and `Acceptance Criteria` in `/specs/001-ephemeral-media-messaging/spec.md`.  
- Security: Threat model and cryptography choices documented in `research.md` (Phase 0). All cryptographic primitives will rely on audited crates and follow Signal's X3DH + Double Ratchet model; secrets and configuration loaded from an external secret store (ENV-backed with explicit guidance). Any new dependencies will be listed for security review.  
- Privacy: Privacy impact and retention policy mirrors spec (burn-on-view + 30-day expiry). Server stores only minimal routing metadata and encrypted blobs. See `Zero-Knowledge Metadata Minimization` and `Assumptions` in spec.md.  
- Performance: Performance goals defined above (2s delivery target, 10k concurrent). Monitoring guidance: instrumented metrics via Prometheus (expose `/metrics`) and structured logs via `tracing`. Load-testing plan and benchmarks will be specified in Phase 2 tasks.

The plan links to the feature spec (`spec.md`) for verification. If any gate cannot be satisfied before Phase 0, the plan will call out the gap and provide a mitigation path in `research.md`.

## Project Structure

### Documentation (this feature)

```text
specs/[###-feature]/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

Selected structure: Single Rust backend crate (server binary) laid out at repository root. Key paths:

```text
Cargo.toml                # workspace or server crate manifest
src/
├── main.rs               # starts axum + tokio runtime
├── routes/               # HTTP + WebSocket route handlers
├── services/             # business logic (messaging, storage, expiry)
├── db/                   # sqlx migrations & DB helpers
├── crypto/               # crypto integrations (X3DH, Double Ratchet wiring)
└── ws/                   # websocket connection manager
migrations/               # sqlx/postgres migration files
tests/
├── integration/
└── contract/
``` 

This structure keeps a single binary service and maps cleanly to the features required (routes, services, crypto, db, ws). Database migrations will live in `migrations/`.

## Complexity Tracking

No Constitution violations identified for this plan. All non-negotiable gates (Code Quality, Testing Standards, Security, Privacy) are addressed in the plan and supporting artifacts (`research.md`, `contracts/openapi.yaml`, `data-model.md`). Any future deviations will be documented here with justification.
