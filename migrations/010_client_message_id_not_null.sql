-- Migration: Make client_message_id NOT NULL
-- We must delete any existing messages with NULL client_message_id first to satisfy the constraint.
DELETE FROM messages WHERE client_message_id IS NULL;

ALTER TABLE messages
ALTER COLUMN client_message_id SET NOT NULL;
