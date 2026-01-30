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
3.  **Encryption**: Client encrypts the backup payload (JSON) using AES-GCM (or similar authenticated encryption).
4.  **Upload**: The encrypted binary blob is sent to the server.

### 2.2 Threat Model
-   **Server Compromise**: Attacker gets opaque blobs. Without the user's password, they are useless.
-   **Weak Passwords**: If a user chooses a weak password, their backup is vulnerable to offline brute-force attacks if the DB is leaked. This is an accepted trade-off for better UX (vs. 12-word phrases).

## 3. Data Schema
The backup payload (inside the encryption) should contain:
-   **Identity KeyPair**: The private/public keys that identify the user.
-   **Contacts List**: Usernames/UUIDs of friends.
-   **Profile Reference**: The `attachment_id` and decryption key for their current Avatar (see Design Doc 002).

## 4. Server Architecture

### 4.1 Storage Limits
To prevent abuse and manage costs:
-   **One-Slot Rule**: Each `user_id` is allowed exactly **one** backup row.
-   New uploads **overwrite** the previous backup.

### 4.2 API Endpoints

#### `POST /v1/backup`
-   **Auth**: Required (Bearer Token).
-   **Body**: Binary blob (Max size limit: e.g., 1MB).
-   **Action**: Upsert blob into `backups` table.

#### `GET /v1/backup`
-   **Auth**: Required.
-   **Response**: The binary blob.

## 5. Implementation Tasks
- [ ] Create `backups` table (`user_id` PK, `blob` BYTEA, `updated_at` TIMESTAMP).
- [ ] Implement API endpoints.
- [ ] Write integration tests verifying overwrite behavior.
