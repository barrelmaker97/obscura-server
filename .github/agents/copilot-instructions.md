# obscura-server Development Guidelines

Auto-generated from all feature plans. Last updated: 2025-12-21

## Active Technologies

- Rust (stable toolchain; minimum Rust 1.70; target latest stable) + `tokio` (async runtime), `axum` (HTTP + WebSocket endpoints), `sqlx` (Postgres async DB access with compile-time checks), `aws-sdk-s3` (S3-compatible object storage with endpoint override) or `rusoto` alternative if needed, `serde`/`serde_json` for serialization, `tracing` for structured logging. Cryptography: use vetted Signal-like libraries or explicit integration notes (X3DH + Double Ratchet primitives implemented in audited crates; more in `research.md`). (001-ephemeral-media-messaging)

## Project Structure

```text
src/
tests/
```

## Commands

cargo test
cargo clippy
cargo fmt -- --check

## Code Style

Rust (stable toolchain; minimum Rust 1.70; target latest stable): Follow standard conventions

## Recent Changes

- 001-ephemeral-media-messaging: Added Rust (stable toolchain; minimum Rust 1.70; target latest stable) + `tokio` (async runtime), `axum` (HTTP + WebSocket endpoints), `sqlx` (Postgres async DB access with compile-time checks), `aws-sdk-s3` (S3-compatible object storage with endpoint override) or `rusoto` alternative if needed, `serde`/`serde_json` for serialization, `tracing` for structured logging. Cryptography: use vetted Signal-like libraries or explicit integration notes (X3DH + Double Ratchet primitives implemented in audited crates; more in `research.md`).

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
