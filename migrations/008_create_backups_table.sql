CREATE TABLE backups (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    current_version INT NOT NULL DEFAULT 0,
    pending_version INT,
    state VARCHAR NOT NULL DEFAULT 'ACTIVE',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    pending_at TIMESTAMPTZ
);

CREATE INDEX idx_backups_state_pending_at ON backups(state, pending_at);
