# Design Doc 004: Push Notifications (FCM)

## 1. Overview
When a user is not connected to the WebSocket (offline), they need to be notified of incoming messages so they can wake up the app and fetch content.

## 2. Privacy & Strategy
-   **Payload:** "Generic" notification only (`"New Message"`).
-   **Metadata:** NO sender names, NO message snippets, NO user IDs sent to Google/Apple servers.
-   **Provider:** Firebase Cloud Messaging (FCM) v1 API.

## 3. Database Schema
We strictly enforce a **Single Device** policy.

### New Table: `push_tokens`
| Column | Type | Constraints |
| :--- | :--- | :--- |
| `user_id` | UUID | PK, FK -> `users(id)` |
| `token` | VARCHAR | NOT NULL |
| `updated_at` | TIMESTAMPTZ | DEFAULT NOW() |

*Constraint:* Since `user_id` is the Primary Key, a user can only ever have **one** registered push token. New logins overwrite the old one.

## 4. API Endpoints

### `PUT /v1/push/token`
-   **Auth**: Required.
-   **Body**: `{ "token": "fcm_token_string" }`
-   **Behavior**: Upserts into `push_tokens`.

## 5. Backend Logic (MessageService)
1.  **Check Connection**: Try to send envelope via WebSocket `Notifier`.
2.  **If Offline**:
    -   Lookup `token` from `push_tokens` for `recipient_id`.
    -   If found, send HTTP/2 POST to FCM API.
3.  **Error Handling**:
    -   If FCM returns `UNREGISTERED` (404) or `INVALID_ARGUMENT`, **delete** the token from the DB immediately (stale token cleanup).

## 6. Implementation Notes
-   Use `reqwest` + `google_auth` (or manual JWT signing) to talk to FCM HTTP v1.
-   Do not use heavy third-party FCM crates if possible; the API is simple.
