# Implementation Plan: Device Takeover

**Branch**: `002-device-takeover` | **Date**: 2026-01-13 | **Spec**: [specs/002-device-takeover/spec.md](specs/002-device-takeover/spec.md)
**Input**: Feature specification from `/specs/002-device-takeover/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

Enable "Device Takeover" to enforce a "Single Active Device" strategy. This involves allowing an authenticated user to overwrite their existing Identity Key, which triggers a cascade of cleanup actions: deleting old pre-keys and pending messages, and forcibly disconnecting any active WebSocket connections associated with the old key.

## Technical Context

**Language/Version**: Rust 1.75+
**Primary Dependencies**: Axum (Web), SQLx (Postgres), Tokio (Async/Broadcast), DashMap (State)
**Storage**: PostgreSQL (Relational Data)
**Testing**: `cargo test` (Unit/Integration with `sqlx::test`)
**Target Platform**: Linux server
**Project Type**: Single API Server
**Performance Goals**: <200ms API response for takeover; Immediate WebSocket disconnection.
**Constraints**: Zero-Knowledge (server cannot decrypt), Single Device (strict 1:1 user-key mapping).
**Scale/Scope**: Core Identity logic update.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

This project enforces the repository Constitution (see
`.specify/memory/constitution.md`). Before Phase 0 research completes, the
plan MUST include evidence for the following gates:

- **Code Quality**: Changes will follow existing module structure (`api/keys.rs`, `storage/key_repo.rs`). Refactoring `InMemoryNotifier` to be strongly typed improves quality.
- **Testing Standards**: New integration tests in `tests/integration_takeover.rs` will cover the full takeover flow (Overwrite -> Verify Delete -> Verify Disconnect). Unit tests for new Repo methods.
- **Security**: "Key before Connect" policy enforced in `websocket_handler`. Authenticated endpoint for takeover.
- **Privacy**: Old pending messages are deleted (enhancing forward secrecy). No new PII.
- **Performance**: Database deletions (messages, pre-keys) will be batched/transactional.

## Project Structure

### Documentation (this feature)

```text
specs/002-device-takeover/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output
```

### Source Code (repository root)

```text
src/
├── api/
│   ├── keys.rs          # Update: Modify upload_keys to accept optional identity_key
│   └── gateway.rs       # Update: Check identity key, handle disconnect
├── core/
│   └── notification.rs  # Refactor: Support specific event types (Disconnect)
└── storage/
    ├── key_repo.rs      # Update: delete_all_pre_keys
    └── message_repo.rs  # Update: delete_all_pending

tests/
└── integration_takeover.rs # New: Verify full lifecycle
```

**Structure Decision**: Option 1: Single project (DEFAULT)

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Refactor Notifier | Support "Disconnect" event | Using a separate channel map is messy/race-prone |