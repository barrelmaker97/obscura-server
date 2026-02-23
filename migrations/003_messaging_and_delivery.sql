-- Messages
-- Includes client_message_id for sender-side deduplication.
-- Using uuidv7() for server-generated ID for time-ordered clustering.
CREATE TABLE messages (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    sender_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recipient_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_message_id UUID NOT NULL,
    message_type INTEGER NOT NULL DEFAULT 2, -- 2 = Encrypted Message
    content BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- Deduplication constraint: A sender cannot send the same client_message_id twice.
    UNIQUE (sender_id, client_message_id)
);

CREATE INDEX idx_messages_fetch ON messages(recipient_id, created_at);

-- Attachments
CREATE TABLE attachments (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_attachments_expires_at ON attachments(expires_at);

-- Push Tokens
CREATE TABLE push_tokens (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    token TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_push_tokens_updated_at ON push_tokens(updated_at);

-- Backups
CREATE TABLE backups (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    current_version INT NOT NULL DEFAULT 0,
    pending_version INT,
    state VARCHAR NOT NULL DEFAULT 'ACTIVE', -- 'ACTIVE', 'PENDING', 'FAILED'
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    pending_at TIMESTAMPTZ
);

CREATE INDEX idx_backups_state_pending_at ON backups(state, pending_at);
