# Data Model: Device Takeover

## Logical Entities

### UserEvent (New Enum)
Used in `InMemoryNotifier` to signal specific events to active clients.

| Variant | Payload | Description |
|---------|---------|-------------|
| `MessageReceived` | None | Signals the client to fetch pending messages from the inbox. |
| `Disconnect` | None | Signals the active WebSocket connection to terminate immediately. |

## Storage Entities (PostgreSQL)

*No schema changes required. Existing tables are sufficient.*

### Transactional Logic (Takeover)

The "Takeover" operation is a complex transaction involving multiple tables:

1.  **`identity_keys`**: Update `identity_key` for `user_id`.
2.  **`signed_pre_keys`**: Delete ALL for `user_id` -> Insert NEW.
3.  **`one_time_pre_keys`**: Delete ALL for `user_id` -> Insert NEW.
4.  **`messages`**: Delete ALL where `recipient_id` = `user_id` AND `expires_at` > NOW.

## Validation Rules

- **Identity Key**: Must be a valid format (Base64 encoded, likely 32 bytes for X25519).
- **PreKeys**: Must include at least one Signed PreKey and a batch of One-Time PreKeys.
