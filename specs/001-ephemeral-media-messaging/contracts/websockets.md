# WebSocket API Contract

This document specifies the server-to-client messages sent over the WebSocket connection. The client is expected to maintain a single, persistent WebSocket connection after authenticating.

## Connection

-   **Endpoint**: `/v1/ws`
-   **Authentication**: The WebSocket handshake request must include the same authentication bearer token as standard HTTP requests (`Authorization: Bearer <token>`).

## Message Format

All messages from the server are JSON objects with a `type` field indicating the event.

```json
{
  "type": "event_name",
  "payload": { ... }
}
```

---

## Server-to-Client Messages

### 1. New Message Notification

-   **Type**: `new_message`
-   **Description**: Sent to a recipient when a new message has been successfully delivered and is ready for download.
-   **Payload**:
    ```json
    {
      "message_id": "uuid-v4-string",
      "sender_username": "string",
      "sent_at": "string (ISO 8601 timestamp)",
      "content": "base64-string (Signal envelope)",
      "blob_id": "uuid-string (optional)"
    }
    ```

<!-- 
Note: The server does not push "receipt_update" events. 
Receipts are handled as standard messages sent from Recipient -> Sender.
-->
