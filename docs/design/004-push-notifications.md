# Design Doc 004: Push Notifications (FCM)

## 1. Overview
When a user is not connected to the WebSocket (offline), they need to be notified of incoming messages so they can wake up the app and fetch content.

## 2. Privacy & Strategy (The Signal Model)
To support **Sealed Sender** (where the server doesn't know who sent the message), we cannot send "Message from Alice" in the push.

### 2.1 Payload Type: "Wake Up"
-   **Android**: FCM "Data Message" with `priority: 'high'`.
-   **iOS**: APNs "VoIP Push" (PushKit) or Background Notification with `apns-priority: '10'`.
-   **Behavior**:
    1.  Push wakes app in background immediately.
    2.  App connects to WebSocket -> Fetches Sealed Envelope.
    3.  App decrypts Sender Identity locally.
    4.  App posts **Local Notification**: "Message from Alice".

### 2.2 Metadata
-   `collapse_key`: `"obscura_check"` (Deduplicates wake-up calls).

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

### 5.1 Refactoring `Notifier` Trait
The `Notifier::notify` method currently returns `()`. We must change it to return `bool`:
-   `true`: User is connected (WebSocket active).
-   `false`: User is offline (No active subscribers).

### 5.2 Flow
1.  **Store Message**: `repo.create(...)`.
2.  **Attempt Local Delivery**: `let online = notifier.notify(recipient_id, ...);`
3.  **If Offline (`!online`)**:
    -   Lookup `token` from `push_tokens`.
    -   If found, send `Data Message` to FCM.
4.  **Error Handling**:
    -   If FCM returns `UNREGISTERED`, delete token from DB.

## 6. Implementation Notes
-   Use `reqwest` + `google_auth` (or manual JWT signing) to talk to FCM HTTP v1.
-   Do not use heavy third-party FCM crates if possible; the API is simple.
