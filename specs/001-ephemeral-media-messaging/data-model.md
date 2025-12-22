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
  - `registered_at` TIMESTAMP WITH TIME ZONE DEFAULT now()
  - `active` BOOLEAN DEFAULT TRUE
- Notes: MVP enforces one active device per user.

3. CiphertextBlob
- Table: `ciphertext_blobs`
- Fields:
  - `id` UUID PRIMARY KEY
  - `storage_pointer` TEXT NOT NULL -- S3 object key
  - `size_bytes` BIGINT
  - `checksum` TEXT
  - `created_at` TIMESTAMP WITH TIME ZONE DEFAULT now()

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
- messages.status: `queued` -> `delivered` -> `read`.
- On `read`: server issues deletion of S3 object and removes `ciphertext_blob` record (or marks it deleted and purges later).

## Indexes
- `messages(recipient_id, status, created_at)` for fast scan of queued messages
- `ciphertext_blobs(created_at)` for expiry sweeper
- `users(username)` unique index

## Migrations
- Use `sqlx` migrations under `migrations/` with up/down SQL files.
