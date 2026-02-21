-- Add client_message_id and client_timestamp_ms to messages table
-- This supports message deduplication and UI sorting (sender-provided).
ALTER TABLE messages ADD COLUMN client_message_id UUID;
ALTER TABLE messages ADD COLUMN client_timestamp_ms BIGINT;

-- Ensure uniqueness of client_message_id per sender for deduplication
CREATE UNIQUE INDEX idx_messages_sender_client_id ON messages(sender_id, client_message_id);

-- Add index for potential sorting by client timestamp if needed
CREATE INDEX idx_messages_client_timestamp ON messages(client_timestamp_ms);
