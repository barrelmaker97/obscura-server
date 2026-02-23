-- Identity Keys
CREATE TABLE identity_keys (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    identity_key BYTEA NOT NULL,
    registration_id INTEGER NOT NULL
);

-- Signed Pre-Keys
CREATE TABLE signed_pre_keys (
    id INTEGER NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,
    signature BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    PRIMARY KEY (id, user_id)
);

-- One-Time Pre-Keys
CREATE TABLE one_time_pre_keys (
    id INTEGER NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    PRIMARY KEY (id, user_id)
);
