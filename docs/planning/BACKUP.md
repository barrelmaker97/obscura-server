# Encrypted Cloud Backup & Storage Refactor

## 1. Overview
This feature allows users to recover their account identity and contacts list when switching to a new device. It explicitly **excludes** message history, aligning with Obscura's ephemeral nature.

As part of this work, the server's storage layer will be refactored to use a generic **ObjectStorage** adapter and **Namespaced Prefixes** (folders) in S3.

## 2. Security Model: "Dumb Cloud"
The server acts as a dumb storage bucket. It has zero knowledge of the encryption keys or the contents of the backup.

### 2.1 Encryption Strategy (Client-Side)
- **User Input**: Strong password (enforced > 12 chars).
- **Key Derivation**: **Argon2id**.
    - *Parameters*: Tuned to take ~500ms - 1s on standard mobile devices.
    - *Output*: 256-bit AES Key.
- **Serialization**: **Protobuf** (not JSON) for compactness and type safety.
- **Encryption**: **AES-GCM** (256-bit).
- **Upload**: The encrypted binary blob is sent to the server.

### 2.2 Threat Model
- **Server Compromise**: Attacker gains opaque blobs. Useless without the user's password.
- **Weak Passwords**: Vulnerable to offline brute-force if the DB is leaked.
- **Recovery**: None. Losing the password means losing the identity.

## 3. Storage Layer Refactor (Foundational)

To support backups while maintaining the existing attachment system, we are introducing a generic **ObjectStorage** trait and moving to a prefix-based bucket structure.

### 3.1 The `ObjectStorage` Port
All storage interactions will move through a generic trait in `src/adapters/storage/mod.rs`.
- **Portability**: Allows swapping S3 for local disk (testing) or other providers.
- **S3 Implementation**: The `aws_sdk_s3::Client` and the complex `mpsc` streaming bridge logic will be moved from `AttachmentService` into a dedicated `S3Storage` adapter.

### 3.2 Namespace Strategy (Breaking Change)
To enable different lifecycle policies, we are moving to a folder-based structure.
- **Attachments**: `attachments/` prefix (Auto-delete after 30 days).
- **Backups**: `backups/` prefix (Persistent).
- **Avatars**: `avatars/` prefix (Persistent).

**Note:** This is a **breaking change**. Any existing attachments at the bucket root will become inaccessible.

## 4. Backup Architecture: "Pending/Commit" Workflow

To prevent database connection pool starvation and ensure transactional integrity between Postgres and S3, we use a multi-phase internal orchestration.

### 4.1 Components
- **`BackupService` (Orchestrator)**: Manages the business logic and coordinates between the repository and storage.
- **`BackupRepository` (Metadata)**: Postgres adapter handling versioning, state, and locking.

### 4.2 Database Schema (`backups` table)
| Column | Type | Description |
| :--- | :--- | :--- |
| `user_id` | UUID (PK) | References `users.id`. |
| `current_version` | INT | The last successfully committed backup version. |
| `pending_version` | INT (Null) | The version currently being uploaded. |
| `state` | VARCHAR | `ACTIVE` or `UPLOADING`. |
| `updated_at` | TIMESTAMPTZ | Last successful commit. |
| `pending_at` | TIMESTAMPTZ | When the current upload started (for Janitor). |

### 4.3 The Atomic Swap Workflow (Server-Side)
When a client calls `POST /v1/backup`:

1. **Reservation**:
    - Acquire DB connection.
    - `SELECT FOR UPDATE` on user's backup row.
    - Verify `If-Match` version (Optimistic Locking). Return **`412 Precondition Failed`** on mismatch.
    - Set `state = 'UPLOADING'`, `pending_version = current + 1`, `pending_at = NOW()`.
    - **Release DB connection** back to the pool.
2. **Streaming**:
    - Stream bytes to `ObjectStorage` using a versioned key: `backups/{user_id}/v{pending_version}`.
    - Enforce request timeout (e.g., 60s).
3. **Finalization**:
    - Acquire DB connection.
    - Set `current_version = pending_version`, `pending_version = NULL`, `state = 'ACTIVE'`.
    - **Release DB connection**.
4. **Cleanup**:
    - (Fire-and-forget) Delete the *old* S3 file: `backups/{user_id}/v{old_version}`.

## 5. Reliability & Edge Cases

### 5.1 The "Janitor" Task
A background worker runs every 5–10 minutes to clean up "Zombie" uploads (S3 success but DB failure, or abandoned uploads).
- **Query**: Find rows where `state = 'UPLOADING'` and `pending_at < (NOW() - 30 minutes)`.
- **Action**: Delete the `pending_version` file from S3 and reset the DB row to `ACTIVE`.

### 5.2 Pre-empting Failed Uploads
If a user tries to upload while a row is already in `UPLOADING` state:
- If `pending_at` is recent (< 60s), return `409 Conflict`.
- If `pending_at` is old, allow the new request to "take over" the slot (update `pending_at` and reuse the `pending_version`).

### 5.3 S3 Key Strategy
Always use versioned keys (`backups/{user_id}/v{version}`) to ensure that a failed upload never corrupts the existing "Known Good" backup.

## 6. API Endpoints

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

## 7. Configuration

| Variable | Default | Description |
| :--- | :--- | :--- |
| `OBSCURA_STORAGE_ATTACHMENT_PREFIX` | `attachments/` | S3 prefix for logical namespacing of attachments. |
| `OBSCURA_STORAGE_BACKUP_PREFIX` | `backups/` | S3 prefix for logical namespacing of backups. |
| `OBSCURA_BACKUP_MAX_SIZE_BYTES` | `2097152` | Max backup size (2MB). |
| `OBSCURA_BACKUP_MIN_SIZE_BYTES` | `32` | Min backup size to prevent accidental wipes. |
| `OBSCURA_BACKUP_UPLOAD_TIMEOUT_SECS` | `60` | S3 streaming timeout. |
| `OBSCURA_BACKUP_STALE_THRESHOLD_MINS` | `30` | Grace period for "UPLOADING" state before Janitor cleanup. |
| `OBSCURA_BACKUP_JANITOR_INTERVAL_SECS` | `300` | Frequency of background cleanup worker cycles. |

## 8. Implementation Tasks

### Phase 1: Infrastructure & Storage Refactor
- [ ] Add `BackupConfig` and prefix fields to `src/config.rs`.
- [ ] Create `ObjectStorage` trait in `src/adapters/storage/mod.rs`.
- [ ] Implement `S3Storage` adapter (migrating code from `AttachmentService`).
- [ ] Update `AttachmentService` to use the new adapter and `attachments/` prefix.
- [ ] Update `AttachmentCleanupWorker` to use the new adapter and prefix.
- [ ] Migration: Create `backups` table with `state` and `pending` columns.

### Phase 2: Backup Orchestration
- [ ] Implement `BackupRepository` (Postgres) with `reserve_slot` and `commit_version` logic.
- [ ] Implement `BackupService` with the "Atomic Swap" logic.
- [ ] Add `412 Precondition Failed` and `409 Conflict` error mapping.
- [ ] Refactor `AttachmentService` to use the new `ObjectStorage` trait.
- [ ] Add `BackupCleanupWorker` (The Janitor).

### Phase 3: API & Testing
- [ ] Implement `GET`, `HEAD`, and `POST /v1/backup` endpoints.
- [ ] Support `Expect: 100-continue` logic in the backup handler.
- [ ] Integration test: Verify `attachments/` and `backups/` are correctly namespaced in S3.
- [ ] Integration test: Mock S3 failure during upload and verify DB state remains consistent.
- [ ] Integration test: Verify `412` on version mismatch and `409` on concurrent upload.
- [ ] Integration test: Verify Janitor cleans up stale `UPLOADING` records.
