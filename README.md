# Obscura Server

**Obscura Server** is a minimalist, secure relay server for the Signal Protocol. It facilitates end-to-end encrypted asynchronous messaging while maintaining a zero-knowledge architecture.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![CI](https://github.com/barrelmaker97/obscura-server/actions/workflows/ci.yml/badge.svg)](https://github.com/barrelmaker97/obscura-server/actions/workflows/ci.yml)

## Features

- **Zero-Knowledge Architecture**: The server stores opaque encrypted blobs. It cannot read message content or access private keys.
- **Signal Protocol Support**: Native support for PreKeys (Identity, Signed, One-Time) to enable X3DH key exchanges.
- **Asynchronous Delivery**: Stores encrypted messages until the recipient comes online to fetch them via WebSockets.
- **Encrypted Attachments**: Secure file uploads via S3-compatible storage with automated TTL-based expiry.
- **Strict Privacy Limits**: Automatic garbage collection of old messages and global inbox limits to prevent metadata buildup.
- **Observability Native**: Built-in support for Traces, Metrics, and Structured Logging via OTLP.

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
A `docker-compose.yml` is provided for a complete local stack (Postgres + Valkey + MinIO + Grafana OTEL LGTM + Server):

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
| `--auth-jwt-secret` | `OBSCURA_AUTH_JWT_SECRET` | `change_me_in_production` |
| `--storage-bucket` | `OBSCURA_STORAGE_BUCKET` | `obscura-storage` |

See [**docs/CONFIGURATION.md**](./docs/CONFIGURATION.md) for the full list of over 50 configuration options.

---

## Development

### Prerequisites

**Build:**
- Rust (stable toolchain, see [rust-toolchain.toml](./rust-toolchain.toml))
- `protoc` (Protocol Buffers compiler)
- [`just`](https://github.com/casey/just) (task runner)

**Runtime:**
- PostgreSQL (17+; migrations use `uuidv7()`)
- Valkey (or Redis)
- S3-compatible storage (e.g. MinIO)

> **Tip:** `just services` starts all runtime dependencies via Docker Compose for local development.

### Running Locally
1. Start infrastructure: `just services`
2. Start the server (migrations run automatically):
   ```bash
   export OBSCURA_DATABASE_URL=postgres://user:password@localhost/signal_server
   export OBSCURA_PUBSUB_URL=redis://localhost:6379
   export OBSCURA_AUTH_JWT_SECRET=test
   export OBSCURA_STORAGE_BUCKET=test-bucket
   export OBSCURA_STORAGE_ENDPOINT=http://localhost:9000
   export OBSCURA_STORAGE_ACCESS_KEY=minioadmin
   export OBSCURA_STORAGE_SECRET_KEY=minioadmin
   export OBSCURA_STORAGE_FORCE_PATH_STYLE=true
   cargo run
   ```

### Testing
```bash
just test
```

### Code Coverage
```bash
just coverage       # LCOV report with summary
just coverage-html  # Browsable HTML report in coverage/
```

### Available Commands
Run `just` to see all available recipes:
```bash
just
```

---

## Documentation

- **API Reference**: [openapi.yaml](./openapi.yaml)
- **Configuration Guide**: [docs/CONFIGURATION.md](./docs/CONFIGURATION.md)
- **Release Process**: [docs/RELEASING.md](./docs/RELEASING.md)
- **System Architecture**: [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md)
- **Planning Docs**: [docs/planning/](./docs/planning/)

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
