# Design Doc 002: Profile Management

## 1. Overview
Users need a way to share a display name and avatar with other users without exposing this data to the server in plaintext.

## 2. Architecture: Persistent Attachments

### 2.1 The Problem with Current Attachments
Currently, the `AttachmentService` enforces a strict TTL (e.g., 30 days), after which files are garbage collected (deleted from DB and S3). Avatars need to persist indefinitely until changed.

### 2.2 Solution: Reference Strategy
Instead of a manual `is_persisted` flag (which is prone to sync errors), we will link the avatar directly to the user profile.

1.  **Database Schema**:
    -   Add `avatar_id` (UUID, Nullable) to the `users` table.
    -   FK: `users.avatar_id` references `attachments.id`.
2.  **Garbage Collector (Refined)**:
    -   The GC query becomes: "Delete attachments where `expires_at < NOW()` AND `id` is NOT present in `users.avatar_id`."
    -   This guarantees that as long as an avatar is "in use" by a profile, it is safe. As soon as the user changes it (updates `users.avatar_id`), the old one becomes eligible for GC (assuming it has expired).

### 2.3 Upload Flow (Atomic Swap)
1.  Client uploads new attachment (standard upload, gets standard TTL).
2.  Client calls `PUT /v1/profile` with `{ "avatarId": "new_uuid", ... }`.
3.  Server updates `users` table.
4.  Old avatar (no longer referenced) will eventually expire and be GC'd. No manual deletion logic required.

## 3. Profile Metadata & Key Distribution

### 3.1 Encryption
-   **Avatar**: Image is encrypted with a random AES key (`AvatarKey`) by the client before upload.
-   **Metadata**: Display Name + Avatar `attachment_id` + `AvatarKey` are bundled together.

### 3.2 Key Distribution (Capability-Based)
The server does not control who sees the profile. Access is granted via keys.
-   **Distribution**: User A sends their "Profile Key" to User B (e.g., during the initial handshake or inside the PreKey Bundle).
-   **Access**: User B uses the `attachment_id` to fetch the encrypted blob from S3, then uses the `AvatarKey` to view it.

### 3.3 Compatibility with Sealed Sender
Since the server only stores opaque blobs and `attachment_ids`, it does not need to know *who* is requesting an avatar, only that they are requesting a valid ID.
-   *Future enhancement*: We may need to sign avatar requests to prevent scraping, or rely on the fact that `attachment_id` is a random UUID (hard to guess).

## 4. Implementation Tasks
- [ ] Migration: Add `is_persisted` column to `attachments`.
- [ ] Backend: Update `AttachmentService` to handle persisted uploads.
- [ ] Backend: Update Garbage Collector logic.
- [ ] API: Add logic to manage the "Single Avatar" limit (delete old avatar when new one is uploaded).
