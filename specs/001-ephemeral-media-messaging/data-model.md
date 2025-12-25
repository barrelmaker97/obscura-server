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

4. PhotoMessage
- Table: `messages`
- Fields:
  - `id` UUID PRIMARY KEY
  - `sender_id` UUID REFERENCES users(id)
  - `recipient_id` UUID REFERENCES users(id)
  - `ciphertext_blob_id` UUID REFERENCES ciphertext_blobs(id)
  - `status` TEXT NOT NULL -- enum: queued/delivered/read
  - `created_at` TIMESTAMP WITH TIME ZONE DEFAULT now()
  - `expires_at` TIMESTAMP WITH TIME ZONE -- created_at + interval '30 days'

5. DeliveryReceipt
- Table: `receipts`
- Fields:
  - `id` UUID PRIMARY KEY
  - `message_id` UUID REFERENCES messages(id) ON DELETE CASCADE
  - `status` TEXT NOT NULL -- queued/delivered/read
  - `timestamp` TIMESTAMP WITH TIME ZONE DEFAULT now()

## Validation Rules
- `username`: regex `^[A-Za-z0-9_]{3,30}$`; enforce uniqueness.
- `expires_at`: default `created_at + interval '30 days'`.
- `ciphertext_blobs.size_bytes`: non-negative and validated on upload.

## State Transitions
- A `messages` record is created only after the client has successfully uploaded the ciphertext blob to S3.
- The initial status is `queued`.
- `messages.status`: `queued` -> `delivered` -> `read`.
- On `read`: server issues deletion of the S3 object associated with the `ciphertext_blob` and removes the database records.

## Indexes
- `messages(recipient_id, status, created_at)` for fast scan of queued messages
- `ciphertext_blobs(created_at)` for expiry sweeper
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