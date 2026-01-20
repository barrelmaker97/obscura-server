CREATE TABLE attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_attachments_expires_at ON attachments(expires_at);
