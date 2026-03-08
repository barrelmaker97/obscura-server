CREATE TABLE identity_keys (
    device_id UUID PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    identity_key BYTEA NOT NULL,
    registration_id INTEGER NOT NULL
);

CREATE TABLE signed_pre_keys (
    id INTEGER NOT NULL,
    device_id UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,
    signature BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    PRIMARY KEY (device_id, id)
);

CREATE TABLE one_time_pre_keys (
    id INTEGER NOT NULL,
    device_id UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    PRIMARY KEY (device_id, id)
);
