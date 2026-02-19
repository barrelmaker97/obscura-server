# Push Notifications: Pending Tasks

While the background worker, Redis-based job leasing, and token management are fully implemented, the actual delivery to mobile devices is currently using a logger stub.

## 1. Real FCM Provider Implementation
The `FcmPushProvider` in `src/adapters/push/fcm.rs` must be updated to communicate with the Google FCM HTTP v1 API.

- [ ] **OAuth2 Authentication**: Implement logic to fetch and cache Google OAuth2 access tokens using a Service Account JSON key.
- [ ] **Request Logic**: Use `reqwest` to send the "Wake Up" payload to `https://fcm.googleapis.com/v1/projects/{project_id}/messages:send`.
- [ ] **Payload Construction**: 
    - Ensure the message is a "Data Message" (no `notification` object) to trigger background execution on Android.
    - Include the `collapse_key: "obscura_check"` to prevent duplicate wake-up signals for the same user.
- [ ] **Error Mapping**: Map specific FCM responses to `PushError` variants:
    - `UNREGISTERED` or `NOT_FOUND` -> `PushError::Unregistered`.
    - `429 Too Many Requests` -> `PushError::QuotaExceeded`.

## 2. Configuration Expansion
The `Config` struct in `src/config.rs` needs new fields to support the FCM client.

| Variable | Description |
| :--- | :--- |
| `OBSCURA_FCM_PROJECT_ID` | The Google Cloud Project ID. |
| `OBSCURA_FCM_CREDENTIALS_JSON` | Path to the service account JSON file or the raw JSON string. |

## 3. End-to-End Validation
- [ ] **Mock FCM Server**: Create a mock HTTP server in the test suite to simulate FCM responses.
- [ ] **Integration Test**: Write a test in `tests/integration_push_worker.rs` that:
    1. Registers a token for User A.
    2. Sends a message to User A.
    3. Advances time or waits for the `PushNotificationWorker` to poll.
    4. Asserts that the mock FCM server received the correct "Wake Up" payload.
