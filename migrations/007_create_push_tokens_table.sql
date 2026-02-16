-- Create push_tokens table for managing device wake-up signals
CREATE TABLE push_tokens (
    -- Single device policy: user_id is the primary key
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    
    -- The FCM/APNS device token
    token TEXT NOT NULL,
    
    -- Used for pruning stale tokens from users who uninstalled the app
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for cleaning up stale tokens by the background worker
CREATE INDEX idx_push_tokens_updated_at ON push_tokens(updated_at);
