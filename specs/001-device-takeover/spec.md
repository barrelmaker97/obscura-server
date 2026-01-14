# Feature Specification: Device Takeover

**Feature Branch**: `001-device-takeover`
**Created**: 2026-01-13
**Status**: Draft
**Input**: User description: "Update the application API to handle 'Device Takeovers' to fully enable a 'Single Active Device' strategy. An authenticated user must be able to overwrite their existing identity key."

## Clarifications

### Session 2026-01-13
- Q: What happens to undelivered messages encrypted for the old key? → A: Delete them immediately (Server wipes undelivered messages for this user upon key update).
- Q: How should the old client be notified or handled? → A: Disconnect immediately. The server must disconnect the old client's websocket and reject any new websocket connections for users without an uploaded identity key.
- Q: What happens to existing Pre-keys (Signed and One-Time) during a takeover? → A: Delete immediately. Both signed pre-keys and one-time pre-keys are considered invalid and must be deleted or replaced upon identity key update.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Device Takeover (Priority: P1)

As an authenticated user, I want to register my new device as my sole active device, overwriting any existing identity key, so that I can resume using the service without manually unregistering my lost or old device.

**Why this priority**: Core functionality to enable the "Single Active Device" strategy and recover from lost devices.

**Independent Test**: Can be tested by simulating a user with an existing key, authenticating from a "new" client, submitting a new key, and verifying the old key is gone and the new one is active.

**Acceptance Scenarios**:

1. **Given** a user has an existing identity key (Key A) stored on the server,
   **When** the user authenticates and submits a new identity key (Key B),
   **Then** the system replaces Key A with Key B.
2. **Given** a user has Key B active,
   **When** the user attempts to send a message,
   **Then** the system uses Key B for operations.
3. **Given** the takeover is complete,
   **When** a sender tries to message the user,
   **Then** the system provides Key B as the user's identity key.
4. **Given** a user has an active WebSocket connection with Key A,
   **When** a takeover request with Key B is successfully processed,
   **Then** the system immediately terminates the Key A WebSocket connection.
5. **Given** a user has existing pre-keys (signed or one-time) on the server,
   **When** an identity key takeover occurs,
   **Then** the server deletes all existing pre-keys associated with that user.

### Edge Cases

- **Concurrent Takeovers**: Two devices attempting takeover simultaneously should result in a consistent state (e.g., last write wins), ensuring only one key remains.
- **Same Key Submission**: Submitting the currently active key should be handled gracefully (idempotent success).
- **Unauthenticated/Invalid Auth**: Requests without valid authentication must be rejected to prevent unauthorized account hijacking.
- **No Identity Key**: Users attempting to connect to the WebSocket without an identity key must be rejected.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide an API endpoint to register an identity key.
- **FR-002**: The system MUST require valid authentication for the key registration endpoint.
- **FR-003**: When a new identity key is submitted for a user, the system MUST overwrite any existing identity key for that user.
- **FR-004**: The system MUST ensure only one identity key is stored per user at any given time (Single Active Device enforcement).
- **FR-005**: The system MUST return a success response upon successful key replacement.
- **FR-006**: The system MUST delete any pending (undelivered) messages associated with the previous identity key upon successful takeover, as they are no longer decryptable.
- **FR-007**: The system MUST immediately disconnect any active WebSocket connection associated with the user upon successful takeover.
- **FR-008**: The system MUST reject new WebSocket connection attempts from authenticated users who do not have an active registered identity key.
- **FR-009**: The system MUST delete all existing Signed Pre-keys and One-Time Pre-keys associated with the user upon successful identity key takeover.

### Constitution Alignment (mandatory)

- **Security**:
    - **Authentication**: Strict authentication checks required before key modification to prevent unauthorized account takeover.
    - **Threat Model**: Mitigates "lost device" denial of service. Enforces "Key before Connect" policy to reduce attack surface.
- **User Privacy**: No additional PII collected. Old keys are replaced.
- **Code Quality**: Implementation must follow existing Rust patterns and project structure.
- **Testing Standards**:
    - Integration tests required to verify the overwrite behavior.
    - Unit tests for the repository/service layer logic.

### Key Entities *(include if feature involves data)*

- **User**: The entity performing the takeover.
- **Identity Key**: The cryptographic public key identifying the user's active device.
- **Pre-keys**: Cryptographic keys (Signed and One-Time) used for session establishment.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can successfully replace an existing identity key via the API.
- **SC-002**: 100% of successful takeover requests result in the new key being the sole active key.
- **SC-003**: System enforces the "Single Active Device" constraint (max 1 key per user) without exception.
- **SC-004**: System successfully blocks 100% of WebSocket connection attempts from users without an identity key.
- **SC-005**: 100% of invalid pre-keys are removed upon identity key update.
