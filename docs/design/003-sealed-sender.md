# Design Doc 003: Sealed Sender (Signal Protocol Standard)

## 1. Overview
Sealed Sender (Unidentified Delivery) allows users to send messages without revealing their identity (`sender_id`) to the server. The server knows *where* the message is going, but not *who* sent it.

## 2. Architecture

### 2.1 Sender Certificates
To prevent abuse (unauthenticated users spamming), the server requires proof of account validity.
1.  **Issuance**: Upon login (or periodically), User A requests a certificate.
2.  **Format**: `Certificate = Sign_Server_PrivKey(User_A_UUID, Device_ID, ExpirationTimestamp)`
3.  **Usage**: User A attaches this certificate to *every* Sealed Sender message.
4.  **Verification**: The server verifies its own signature. It confirms the user is valid, but (crucially) **does not log or inspect** the UUID inside the certificate during routing.

### 2.2 The Envelope Structure
The WebSocket envelope changes significantly.
```protobuf
message SealedEnvelope {
  string destination_uuid = 1;  // Plaintext (for routing)
  bytes sender_identity = 2;    // ENCRYPTED with Recipient's Public Identity Key
  bytes certificate = 3;        // The Server-Signed Certificate
  bytes content = 4;            // The Standard Encrypted Message
  string delivery_token = 5;    // (Optional) Access Control Token
}
```

### 2.3 Sender Identity Hiding
-   The `sender_identity` field contains the sender's UUID.
-   It is encrypted using `SealedBox` (or similar anonymous encryption) against the **Recipient's** Public Identity Key.
-   **Server View**: Cannot read `sender_identity`.
-   **Recipient View**: Decrypts `sender_identity` to know who sent the message.

## 3. Access Control (Blocking)
Since the server cannot see the Sender UUID, it cannot enforce "Block Alice" lists.
We use **Delivery Tokens** (Capability-Based Security).

### 3.1 Unrestricted vs. Restricted Mode
Users can toggle between two modes:
1.  **Unrestricted**: Anyone with a valid Sender Certificate can message me. (Default for new accounts?).
2.  **Restricted**: You must possess my current `DeliveryToken` to message me.

### 3.2 The Delivery Token Flow
1.  **Generation**: User B generates a random 16-byte `DeliveryToken` and uploads a hash of it to the server (`set_delivery_token`).
2.  **Distribution**: User B shares this token with friends (Alice) via the encrypted channel.
3.  **Enforcement**:
    -   Alice sends a message. Includes `delivery_token = "xyz"`.
    -   Server checks: `Hash("xyz") == DB.users[Bob].delivery_token_hash`.
    -   If mismatch: Reject (401).

### 3.3 Blocking Strategy
To block a harasser (Alice):
1.  User B **rotates** their Delivery Token.
2.  User B sends the *new* token to all friends *except* Alice.
3.  Alice tries to send with the old token -> Rejected.
4.  Alice is blocked without the server knowing Alice was the one blocked.

## 4. Implementation Stages

### Phase 1: Sender Certificates
-   Implement `GET /v1/certificate` endpoint.
-   Server signs certificates using a new internal ED25519 key.

### Phase 2: Envelope Update
-   Update Protobufs to support `SealedEnvelope`.
-   Add server logic to verify Certificates on inbound messages.

### Phase 3: Delivery Tokens
-   Add `delivery_token_hash` column to `users`.
-   Implement logic to validate tokens if present.