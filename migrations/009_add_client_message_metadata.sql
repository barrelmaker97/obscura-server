-- Add client_message_id to messages table
-- This supports message deduplication (sender-provided).
ALTER TABLE messages ADD COLUMN client_message_id UUID;

-- Ensure uniqueness of client_message_id per sender for deduplication
CREATE UNIQUE INDEX idx_messages_sender_client_id ON messages(sender_id, client_message_id);
