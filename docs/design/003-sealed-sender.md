# Design Doc 003: Sealed Sender & Privacy

## 1. Overview (Long-Term Goal)
Currently, the server authenticates every request and knows the `sender_id` for every message. This leaks social graph metadata (who is talking to whom).
**Sealed Sender** (based on Signal's Unidentified Delivery) allows users to send messages without the server knowing their identity.

## 2. Architecture Change

### 2.1 The "Sender Certificate"
1.  **Issue**: Server issues a time-limited certificate (token) to User A upon login.
2.  **Send**: User A includes this certificate in the encrypted envelope.
3.  **Verify**: Server verifies the certificate is valid, but the certificate *does not* contain `user_id` in plaintext (or the server is designed not to log/inspect it beyond validity).
    *   *Note*: In a strict Sealed Sender implementation, the sender is truly anonymous to the server. The recipient identifies the sender upon decryption.

### 2.2 Access Control (Blocking)
With Sealed Sender, the server cannot block "User A" from messaging "User B" because it doesn't know who "User A" is.

**Solution: Delivery Tokens (Capabilities)**
1.  **Token Generation**: User B generates a "Delivery Token" (secret string) and shares it with friends (User A).
2.  **Enforcement**: To send a message to User B, User A must attach this valid Delivery Token.
3.  **Blocking**: If User A harasses User B, User B simply **rotates their Delivery Token**.
    -   User B gives the new token to trusted friends.
    -   User A (harasser) still has the old token, which the server now rejects.
    -   Result: User A is blocked without the server knowing User A's identity.

## 3. Impact on Current System
This is a fundamental architectural shift.
-   **Rate Limiting**: Can no longer rate-limit by `sender_id`. Must rely on IP-based limits or anonymous tokens.
-   **Spam**: The "Delivery Token" approach mitigates spam (you can't message someone without their token).

## 4. Implementation Stages
1.  **Phase 1 (Current)**: ID-based routing (Server knows all).
2.  **Phase 2**: Implement "Delivery Tokens" as an optional requirement.
3.  **Phase 3**: Fully strictly Sealed Sender (Sender ID removed from envelope).
