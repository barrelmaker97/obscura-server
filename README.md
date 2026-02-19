# Obscura Server

**Obscura Server** is a minimalist, secure relay server for the Signal Protocol. It facilitates end-to-end encrypted asynchronous messaging while maintaining a zero-knowledge architecture.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![CI](https://github.com/nolan-cooper/obscura-server/actions/workflows/ci.yml/badge.svg)](https://github.com/nolan-cooper/obscura-server/actions/workflows/ci.yml)

## Features

- **Zero-Knowledge Architecture**: The server stores opaque encrypted blobs. It cannot read message content or access private keys.
- **Signal Protocol Support**: Native support for PreKeys (Identity, Signed, One-Time) to enable X3DH key exchanges.
- **Asynchronous Delivery**: Stores encrypted messages until the recipient comes online to fetch them via WebSockets.
- **Encrypted Attachments**: Secure file uploads via S3-compatible storage with automated TTL-based expiry.
- **Strict Privacy Limits**: Automatic garbage collection of old messages and global inbox limits to prevent metadata buildup.
- **Observability Native**: Built-in support for Traces, Metrics, and Structured Logging via OTLP.

---

## Architecture

Obscura acts as a passive relay between Signal Protocol clients. It manages the discovery of cryptographic material and the queuing of encrypted blobs.

```mermaid
sequenceDiagram
    participant A as Alice (Client)
    participant S as Obscura Server
    participant B as Bob (Client)

    Note over A,B: Registration
    A->>S: Upload Identity & PreKeys
    B->>S: Upload Identity & PreKeys

    Note over A,B: Message Exchange
    A->>S: Request Bob's PreKey Bundle
    S-->>A: Bob's Public Keys (Identity, Signed, One-Time)
    A->>A: Perform X3DH (Generate Shared Secret)
    A->>S: Send Encrypted Message (Bob)
    S->>S: Store Opaque Blob
    S-->>B: Real-time Poke (WebSocket/Push)
    B->>S: Fetch Pending Messages
    S-->>B: Bob's Encrypted Blobs
    B->>B: Decrypt with Shared Secret
```

---

## Security & Privacy

### Threat Model
Obscura is designed to protect the **content** of communications even if the server is fully compromised.

- **What the Server Sees**:
    - User IDs and Usernames.
    - Public Keys (Identity, Signed PreKeys, One-Time PreKeys).
    - Metadata: Message timestamps, sizes, and recipient/sender IDs.
    - IP Addresses (unless connected via Tor/VPN).
- **What the Server CANNOT See**:
    - Message plaintext (content).
    - Attachment plaintext.
    - Private keys (they never leave the client).
- **Non-Goals**: Obscura does not currently implement padding or mix-net techniques to protect against advanced traffic analysis or metadata correlation.

---

## Quick Start

### 1. Run with Docker Compose
A `docker-compose.yml` is provided for a complete local stack (Postgres + Valkey + MinIO + Server):

```bash
docker compose up -d
```

### 2. Interaction Example
Once the server is running on port `3000`, you can verify it's working:

```bash
# Check Health (Management Port 9090)
curl http://localhost:9090/readyz

# View OpenAPI Spec
curl http://localhost:3000/openapi.yaml
```

---

## Configuration

Obscura can be configured via command-line flags or environment variables. **Essential configurations** include:

| Option | Environment Variable | Default |
|--------|----------------------|---------|
| `--db-url` | `OBSCURA_DATABASE_URL` | `postgres://user:password@localhost/signal_server` |
| `--pubsub-url` | `OBSCURA_PUBSUB_URL` | `redis://localhost:6379` |
| `--jwt-secret` | `OBSCURA_AUTH_JWT_SECRET` | `change_me_in_production` |
| `--storage-bucket` | `OBSCURA_STORAGE_BUCKET` | `obscura-attachments` |

See [**docs/CONFIGURATION.md**](./docs/CONFIGURATION.md) for the full list of over 50 configuration options.

---

## Development

### Prerequisites
- Rust 1.83+
- PostgreSQL 16+
- Valkey 8+ (or Redis)
- `protoc` (Protocol Buffers compiler)

### Running Locally
1. Start infrastructure: `docker compose up -d db minio valkey`
2. Run migrations and start:
   ```bash
   export OBSCURA_DATABASE_URL=postgres://user:password@localhost/signal_server
   export OBSCURA_PUBSUB_URL=redis://localhost:6379
   export OBSCURA_AUTH_JWT_SECRET=test
   export OBSCURA_STORAGE_BUCKET=test
   cargo run
   ```

### Testing
```bash
cargo test
```

---

## Documentation

- **API Reference**: [openapi.yaml](./openapi.yaml)
- **Configuration Guide**: [docs/CONFIGURATION.md](./docs/CONFIGURATION.md)
- **Release Process**: [docs/RELEASING.md](./docs/RELEASING.md)
- **Design Docs**: [docs/design/](./docs/design/)

# License

Copyright (c) 2026 Nolan Cooper

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.
