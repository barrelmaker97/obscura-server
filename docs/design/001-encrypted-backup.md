# Design Doc 001: Encrypted Cloud Backup

## 1. Overview
This feature allows users to recover their account identity and contacts list when switching to a new device. It explicitly **excludes** message history, aligning with Obscura's ephemeral nature.

## 2. Security Model: "Dumb Cloud"
The server acts as a dumb storage bucket. It has zero knowledge of the encryption keys or the contents of the backup.

### 2.1 Encryption Strategy (Client-Side)
We avoid complex enclave-based solutions (like SGX) in favor of a robust standard cryptographic approach managed by the client.

1.  **User Input**: User provides a strong password (enforce length > 12 chars).
2.  **Key Derivation**: Client uses **Argon2id**.
    -   *Parameters*: Must be tuned to take ~500ms - 1s on a standard mobile device.
    -   *Output*: 256-bit AES Key.
3.  **Serialization**: Payload is serialized using **Protobuf** (not JSON) for compactness and type safety.
4.  **Encryption**: Client encrypts the Protobuf blob using AES-GCM.
5.  **Upload**: The encrypted binary blob is sent to the server.

### 2.2 Threat Model
-   **Server Compromise**: Attacker gets opaque blobs. Without the user's password, they are useless.
-   **Weak Passwords**: If a user chooses a weak password, their backup is vulnerable to offline brute-force attacks if the DB is leaked. This is an accepted trade-off for better UX (vs. 12-word phrases).

## 3. Data Schema
The backup payload (inside the encryption) should contain:
-   **Identity KeyPair**: The private/public keys that identify the user.
-   **Contacts List**: Usernames/UUIDs of friends.
-   **Profile Reference**: The `attachment_id` and decryption key for their current Avatar (see Design Doc 002).

## 4. Server Architecture

### 4.1 Hybrid Storage Model
To ensure transactional integrity while allowing for future scalability (e.g., message history), we use a hybrid approach:
- **PostgreSQL**: Stores metadata and handles optimistic locking.
- **S3**: Stores the opaque encrypted binary blob.

### 4.2 Storage Limits & Concurrency
To prevent abuse and manage costs:
-   **One-Slot Rule**: Each `user_id` is allowed exactly **one** backup. S3 objects are keyed by `backups/{user_id}` and overwritten on update.
-   **Concurrency**: Use Optimistic Locking in Postgres.
    -   The `backups` table tracks a `version` (int).
    -   Client must provide `If-Match: <version>` header when updating.
    -   **Workflow**: 
        1. Fetch version from DB (SELECT FOR UPDATE).
        2. Stream blob to S3.
        3. Update version and metadata in DB.
    -   If version mismatch, server returns `409 Conflict`. Client must fetch, merge, and retry.

### 4.3 API Endpoints

#### `POST /v1/backup`
-   **Auth**: Required (Bearer Token).
-   **Headers**: `If-Match: <version>` (Optional for first upload).
-   **Body**: Binary blob.
    -   *Max Size*: 2MB (Strictly enforced initially).
        -   *Scalability Note*: The S3 backend allows us to easily increase this limit (e.g., to 100MB) in the future to support encrypted message history backups without impacting database performance.
    -   *Min Size*: 32 bytes (Prevent accidental zero-byte wipes).
-   **Action**: Validate version, stream blob to S3, increment version in DB.

#### `GET /v1/backup`
-   **Auth**: Required.
-   **Response**: The binary blob (streamed from S3) + `ETag` header (version).

## 5. Implementation Tasks
- [ ] Create `backups` table (`user_id` PK, `version` INT, `updated_at` TIMESTAMP).
- [ ] Implement `BackupRepository` (Postgres) and `BackupService` (S3 integration).
- [ ] Implement API endpoints with `If-Match` support.
- [ ] Write integration tests verifying overwrite behavior and conflict handling.
