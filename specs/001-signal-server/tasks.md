---
description: "Task list for 001-signal-server"
---

# Tasks: Signal Protocol Relay Server

**Input**: Design documents from `specs/001-signal-server/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: The Constitution requires test-first practices. Tests are included for P1 stories and MUST be added as failing tests before implementation.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [x] T001 Create Rust project structure (`cargo new obscura-server`)
- [x] T002 Update `Cargo.toml` with dependencies (`tokio`, `axum`, `sqlx`, `serde`, `prost`, `tracing`, `argon2`)
- [x] T003 [P] Create `build.rs` to compile `specs/001-signal-server/contracts/obscura.proto`
- [x] T004 [P] Configure `clippy` and `rustfmt` in `rust-toolchain.toml` or `.rustfmt.toml`
- [x] T005 Create `.env.example` with `DATABASE_URL` and `JWT_SECRET` variables

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T006 Setup `src/config.rs` to load environment variables
- [x] T007 [P] Create `src/error.rs` defining global `AppError` and `Result` types
- [x] T008 Setup `src/storage/mod.rs` for database connection pool initialization
- [x] T009 Create migration `001_create_users_table.sql` in `migrations/`
- [x] T010 Create migration `002_create_prekeys_tables.sql` (identity, signed, one-time) in `migrations/`
- [x] T011 Create migration `003_create_messages_table.sql` in `migrations/`
- [x] T012 Run `sqlx migrate run` to apply schema
- [x] T013 [P] Setup `src/api/mod.rs` with basic Axum router and fallback handler
- [x] T014 [P] Setup `src/main.rs` to initialize config, db pool, and start server
- [x] T015 [P] Setup `tests/common.rs` (test helpers) and `src/lib.rs` (for integration tests)

**Checkpoint**: Foundation ready - user story implementation can now begin

---

## Phase 3: User Story 1 - User Registration & Key Publishing (Priority: P1) 🎯 MVP

**Goal**: Users can register, upload keys, and other users can fetch those keys.

**Independent Test**: Register User A -> Upload Keys A -> User B fetches Keys A -> Success.

### Tests for User Story 1

- [x] T016 [P] [US1] Create integration test `tests/integration_registration.rs` for register/upload/fetch flow (assert failure first)

### Implementation for User Story 1

- [x] T017 [P] [US1] Create `User` entity and `PreKeyBundle` structs in `src/core/user.rs`
- [x] T018 [US1] Implement `UserRepository` in `src/storage/user_repo.rs` (insert user, find by username)
- [x] T019 [US1] Implement `KeyRepository` in `src/storage/key_repo.rs` (upsert identity/signed keys, batch insert one-time keys, fetch bundle)
- [x] T020 [P] [US1] Implement password hashing logic in `src/core/auth.rs`
- [x] T021 [US1] Create `POST /v1/accounts` handler in `src/api/auth.rs`
- [x] T022 [US1] Create `PUT /v1/keys` handler in `src/api/keys.rs` (requires auth middleware placeholder)
- [x] T023 [US1] Create `GET /v1/keys/{userId}` handler in `src/api/keys.rs`
- [x] T024 [US1] Implement basic JWT generation and verification in `src/api/auth_middleware.rs`
- [x] T025 [US1] Wire up US1 routes in `src/api/mod.rs`

**Checkpoint**: User Story 1 fully functional.

---

## Phase 4: User Story 2 - Asynchronous Message Exchange (Priority: P1)

**Goal**: Users can send encrypted messages via HTTP and receive them via WebSocket.

**Independent Test**: User A sends msg to B -> B connects WS -> B receives msg -> B acks -> Msg deleted.

### Tests for User Story 2

- [x] T026 [P] [US2] Create integration test `tests/integration_messaging.rs` for send/receive/ack flow (assert failure first)

### Implementation for User Story 2

- [x] T027 [P] [US2] Create `Message` entity in `src/core/message.rs`
- [x] T028 [US2] Implement `MessageRepository` in `src/storage/message_repo.rs` (insert, find pending, delete)
- [x] T029 [US2] Implement `MessageService` in `src/core/message_service.rs` (enforce limits/TTL)
- [x] T030 [US2] Create `POST /v1/messages/{destinationDeviceId}` handler in `src/api/messages.rs`
- [x] T031 [P] [US2] Create WebSocket handler structure in `src/api/gateway.rs`
- [x] T032 [US2] Implement WebSocket auth (query param) extraction in `src/api/gateway.rs`
- [x] T033 [US2] Implement WebSocket loop (push pending messages, handle ACKs) in `src/api/gateway.rs`
- [x] T034 [US2] Wire up US2 routes in `src/api/mod.rs`

**Checkpoint**: User Story 2 fully functional.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [x] T035 Update `README.md` with running instructions
- [x] T036 Run `cargo clippy` and fix any warnings
- [x] T037 Verify strict TTL cleanup job (background task or on-insert check)

---

## Dependencies & Execution Order

### Phase Dependencies

1. **Setup (Phase 1)**: No dependencies.
2. **Foundational (Phase 2)**: Depends on Phase 1. Blocks US1 and US2.
3. **User Story 1 (Phase 3)**: Depends on Phase 2.
4. **User Story 2 (Phase 4)**: Depends on Phase 2 (and logically on US1 for Auth/Users, though code can be written in parallel if mocked).
5. **Polish (Phase 5)**: Depends on US1 and US2.

### Parallel Opportunities

- **Setup**: T003 (Build script), T004 (Toolchain) can run parallel to T001/T002.
- **Foundational**: T007 (Errors), T013 (Router), T014 (Main), T015 (Tests) can run parallel to Storage/Migrations (T008-T012).
- **User Stories**:
    - US1 Models (T017) and Tests (T016) can run parallel.
    - US2 Models (T027) and Tests (T026) can run parallel.
    - US1 and US2 *could* be parallelized by two devs if they agree on shared `User` definitions, but sequential is safer for a single dev/agent.

---

## Implementation Strategy

### MVP Delivery

1. **Setup & Foundation**: Build the skeleton and database.
2. **User Story 1**: Enable registration and key exchange. This is the minimum viable "Signal Server" (even without messaging, it's a key directory).
3. **User Story 2**: Enable actual messaging.
4. **Polish**: Code cleanup and documentation.
