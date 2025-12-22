# Quickstart - Ephemeral Media Messaging (MVP)

This quickstart shows how to run the server locally for development and run basic migrations.

Prerequisites

- Rust toolchain (stable), `cargo` installed
- PostgreSQL (local or docker)
- An S3-compatible service for testing (e.g., LocalStack or Minio)

Environment

Create a `.env` file with the following values (example):

```
DATABASE_URL=postgres://postgres:password@localhost:5432/obscura_dev
S3_ENDPOINT=http://localhost:4566
S3_BUCKET=obscura-dev
AWS_REGION=us-east-1
AWS_ACCESS_KEY_ID=test
AWS_SECRET_ACCESS_KEY=test
JWT_SIGNING_KEY=changeme
```

Run Postgres using Docker (example):

```bash
docker run --rm -p 5432:5432 -e POSTGRES_PASSWORD=password -e POSTGRES_DB=obscura_dev postgres:15
```

Run Local S3 (Minio example):

```bash
docker run --rm -p 9000:9000 -e MINIO_ROOT_USER=test -e MINIO_ROOT_PASSWORD=test minio/minio server /data
```

Migrate DB (using `sqlx` migrations):

```bash
# install sqlx-cli if needed
cargo install sqlx-cli --no-default-features --features postgres
export DATABASE_URL=postgres://postgres:password@localhost:5432/obscura_dev
sqlx migrate run
```

Run server:

```bash
cargo run --bin obscura-server
```

Testing

- Run unit + integration tests:

```bash
cargo test
```

Notes

- The server expects client-side encryption using X3DH + Double Ratchet; presigned upload flow is implemented for encrypted blobs.
- See `specs/001-ephemeral-media-messaging/research.md` for design rationale and `specs/001-ephemeral-media-messaging/data-model.md` for DB layout.
