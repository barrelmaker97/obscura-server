# Encrypted Cloud Backup (Revised)

## 1. Overview
This feature allows users to recover their account identity and contacts list when switching to a new device. It explicitly **excludes** message history, aligning with Obscura's ephemeral nature.

## 2. Security Model: "Dumb Cloud"
The server acts as a dumb storage bucket. It has zero knowledge of the encryption keys or the contents of the backup.

### 2.1 Encryption Strategy (Client-Side)
- **User Input**: Strong password (enforced > 12 chars).
- **Key Derivation**: **Argon2id** (tuned for ~500ms–1s on mobile).
- **Serialization**: **Protobuf** payload.
- **Encryption**: **AES-GCM** (256-bit).
- **Upload**: Encrypted binary blob sent to the server.

### 2.2 Threat Model
- **Server Compromise**: Attacker gains opaque blobs. Useless without the user's password.
- **Weak Passwords**: Vulnerable to offline brute-force if the database is leaked.
- **Recovery**: None. Losing the password means losing the identity.

## 3. Architecture: "Pending/Commit" Workflow

To prevent database connection pool starvation during slow uploads and ensure transactional integrity between Postgres and S3, we use a multi-phase internal orchestration.

### 3.1 Components
- **`BackupService` (Orchestrator)**: Manages the business logic and coordinates between the repository and storage.
- **`BackupRepository` (Metadata)**: Postgres adapter handling versioning, state, and locking.
- **`ObjectStorage` (Blobs)**: Generic trait/adapter for S3 (or Memory for testing).

### 3.2 Database Schema (`backups` table)
| Column | Type | Description |
| :--- | :--- | :--- |
| `user_id` | UUID (PK) | References `users.id`. |
| `current_version` | INT | The last successfully committed backup version. |
| `pending_version` | INT (Null) | The version currently being uploaded. |
| `state` | VARCHAR | `ACTIVE` or `UPLOADING`. |
| `updated_at` | TIMESTAMPTZ | Last successful commit. |
| `pending_at` | TIMESTAMPTZ | When the current upload started (for Janitor). |

### 3.3 The Atomic Swap Workflow (Server-Side)
When a client calls `POST /v1/backup`:

1. **Reservation**:
    - Acquire DB connection.
    - `SELECT FOR UPDATE` on user's backup row.
    - Verify `If-Match` version (Optimistic Locking). Return **`412 Precondition Failed`** on mismatch.
    - Set `state = 'UPLOADING'`, `pending_version = current + 1`, `pending_at = NOW()`.
    - **Release DB connection** back to the pool.
2. **Streaming**:
    - Stream bytes to S3 using a versioned key: `backups/{user_id}/v{pending_version}`.
    - Enforce request timeout (e.g., 60s).
3. **Finalization**:
    - Acquire DB connection.
    - Set `current_version = pending_version`, `pending_version = NULL`, `state = 'ACTIVE'`.
    - **Release DB connection**.
4. **Cleanup**:
    - (Fire-and-forget) Delete the *old* S3 file: `backups/{user_id}/v{old_version}`.

## 4. Reliability & Edge Cases

### 4.1 The "Janitor" Task
A background worker runs every 5–10 minutes to clean up "Zombie" uploads (S3 success but DB failure, or abandoned uploads).
- **Query**: Find rows where `state = 'UPLOADING'` and `pending_at < (NOW() - 30 minutes)`.
- **Action**: Delete the `pending_version` file from S3 and reset the DB row to `ACTIVE`.

### 4.2 Pre-empting Failed Uploads
If a user tries to upload while a row is already in `UPLOADING` state:
- If `pending_at` is recent (< 60s), return `409 Conflict`.
- If `pending_at` is old, allow the new request to "take over" the slot (update `pending_at` and reuse the `pending_version`).

### 4.3 S3 Key Strategy
Always use versioned keys (`backups/{user_id}/v{version}`) to ensure that a failed upload never corrupts the existing "Known Good" backup.

## 5. API Endpoints

#### `GET /v1/backup`
- **Auth**: Required.
- **Response**: Streams `backups/{user_id}/v{current_version}` from S3.
- **Headers**: `ETag` (contains `current_version`).

#### `HEAD /v1/backup`
- **Auth**: Required.
- **Response**: `200 OK` (No body).
- **Headers**: Returns the same `ETag` and `Content-Length` as `GET`.
- **Use Case**: Lightweight check for existence and version mismatch without downloading data.

#### `POST /v1/backup`
- **Auth**: Required.
- **Headers**: 
    - `If-Match: <version>`: Mandatory. Return `412` on mismatch.
    - `Expect: 100-continue`: (Recommended) Allows server to reject stale versions before client sends the body.
- **Body**: Binary blob (2MB Max, 32b Min).
- **Action**: Atomic swap orchestration.

## 6. Implementation Tasks

### Phase 1: Infrastructure
- [ ] Create `ObjectStorage` trait in `src/adapters/storage/mod.rs`.
- [ ] Implement `S3Storage` adapter.
- [ ] Migration: Create `backups` table with `state` and `pending` columns.
- [ ] Implement `BackupRepository` (Postgres) with `reserve_slot` and `commit_version` logic.

### Phase 2: Orchestration
- [ ] Implement `BackupService` with the "Atomic Swap" logic.
- [ ] Add `412 Precondition Failed` and `409 Conflict` error mapping.
- [ ] Refactor `AttachmentService` to use the new `ObjectStorage` trait.
- [ ] Add `BackupCleanupWorker` (The Janitor).

### Phase 3: API & Testing
- [ ] Implement `GET`, `HEAD`, and `POST /v1/backup` endpoints.
- [ ] Support `Expect: 100-continue` logic in the backup handler.
- [ ] Integration test: Mock S3 failure during upload and verify DB state remains consistent.
- [ ] Integration test: Verify `412` on version mismatch and `409` on concurrent upload.
- [ ] Integration test: Verify Janitor cleans up stale `UPLOADING` records.
