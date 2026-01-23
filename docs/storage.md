# Storage Layout

## Keys
Public keys (Identity Keys, Signed Pre-Keys, One-Time Pre-Keys) are stored as **33-byte BYTEA** blobs.
These blobs include the Signal protocol prefix (0x05) followed by the 32-byte Montgomery (X25519) public key.

This ensures the database is protocol-faithful and self-describing.
