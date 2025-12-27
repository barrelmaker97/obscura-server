# Data Model

This file extracts entities from the feature spec and maps them to DB tables, fields, and validation rules.

## Entities

1. User
- Table: `users`
- Fields:
  - `id` UUID PRIMARY KEY
  - `username` TEXT UNIQUE NOT NULL -- 3-30 chars, alphanumeric and underscores
  - `created_at` TIMESTAMP WITH TIME ZONE DEFAULT now()

2. Device
- Table: `devices`
- Fields:
  - `id` UUID PRIMARY KEY
  - `user_id` UUID REFERENCES users(id) ON DELETE CASCADE
  - `device_public_key` TEXT NOT NULL -- base64 or PEM
  - `registration_id` INTEGER NOT NULL -- Signal registration id
  - `identity_key` BYTEA NOT NULL -- long-term identity public key (raw bytes)
  - `registered_at` TIMESTAMP WITH TIME ZONE DEFAULT now()
  - `active` BOOLEAN DEFAULT TRUE
 - Notes: MVP enforces one active device per user. The `registration_id` and
   `identity_key` are required to support Signal protocol bundles and to
   verify device-signed requests. `identity_key` is stored as raw bytes; API
   surfaces should accept/return Base64 when convenient.

3. SignedPreKey
- Table: `signed_pre_keys`
- Fields:
  - `id` SERIAL PRIMARY KEY
  - `device_id` UUID REFERENCES devices(id) ON DELETE CASCADE
  - `key_id` INTEGER NOT NULL
  - `public_key` BYTEA NOT NULL
  - `signature` BYTEA NOT NULL
  - `created_at` TIMESTAMP WITH TIME ZONE DEFAULT now()
  - UNIQUE(device_id, key_id)

  Notes: Each device holds one active signed pre-key; the server stores the
  public key and its signature. The server must not hold any private key
  material.

4. OneTimePreKey
- Table: `one_time_pre_keys`
- Fields:
  - `id` SERIAL PRIMARY KEY
  - `device_id` UUID REFERENCES devices(id) ON DELETE CASCADE
  - `key_id` INTEGER NOT NULL
  - `public_key` BYTEA NOT NULL
  - UNIQUE(device_id, key_id)

  Notes: One-time pre-keys are intended to be consumed (deleted) when a
  client fetches a PreKey bundle for initiating a session. The server must
  perform the fetch-and-delete atomically to ensure keys are not reused.
  **Replenishment**: The client is responsible for periodically checking its
  remaining key count and uploading new keys to the server via the API.

5. CiphertextBlob
- Table: `ciphertext_blobs`
- Fields:
  - `id` UUID PRIMARY KEY
  - `storage_pointer` TEXT NOT NULL -- S3 object key
  - `size_bytes` BIGINT
  - `checksum` TEXT
  - `created_at` TIMESTAMP WITH TIME ZONE DEFAULT now()

  Notes: With the redesigned send flow, it is possible for a client to
  upload a blob but fail to send the associated message, resulting in an
  orphaned S3 object. An S3 bucket lifecycle policy should be configured to
  automatically delete objects in the upload prefix after a short time
  (e.g., 24 hours) to mitigate this.

4. Message
- Table: `messages`
- Fields:
  - `id` UUID PRIMARY KEY
  - `sender_id` UUID REFERENCES users(id)
  - `recipient_id` UUID REFERENCES users(id)
  - `content` BYTEA NOT NULL -- The opaque Signal Protocol envelope (headers + encrypted metadata).
  - `associated_blob_id` UUID -- Nullable. If present, indicates an S3 object (key=`associated_blob_id`) exists and must be deleted when this message is deleted.
  - `created_at` TIMESTAMP WITH TIME ZONE DEFAULT now()
  - `expires_at` TIMESTAMP WITH TIME ZONE -- created_at + interval '30 days'

## Validation Rules
- `username`: regex `^[A-Za-z0-9_]{3,30}$`; enforce uniqueness.
- `expires_at`: default `created_at + interval '30 days'`.
- `content`: Max size limit (e.g., 16KB) to prevent abuse of the DB for large storage.

## State Transitions
- **Queued**: Message is created.
- **ACK/Delete**: Client calls `DELETE /messages/:id`.
    - Server deletes the row from `messages`.
    - IF `associated_blob_id` is not null, Server asynchronously deletes the object from S3.
- **Expiry**: Background sweeper deletes messages > 30 days old (and their blobs).

## Indexes
- `messages(recipient_id, created_at)` for fast scan of queued messages for a user
- `users(username)` unique index

## Migrations
- Use `sqlx` migrations under `migrations/` with up/down SQL files.

## Signal Key Store (Signal Protocol Requirements)
To support the official Signal protocol (X3DH + Double Ratchet) the server
must store a small set of public key material per registered device: a
registration id, the long-term identity public key, one active signed pre-key,
and a set of disposable one-time pre-keys. The server never stores private
keys.

Notes:
- One-time pre-keys are intended to be consumed (deleted) when a client
  fetches a PreKey bundle for initiating a session. The server must perform
  the fetch-and-delete atomically to ensure keys are not reused.
- `identity_key` and `registration_id` are required to verify device-signed
  requests and to construct PreKey bundles returned to initiating clients.
- All public keys are stored as `BYTEA` (raw bytes). Use Base64 on API
  surfaces where convenient.