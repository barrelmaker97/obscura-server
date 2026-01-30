# Operational Hardening Plan

This plan details "low-hanging fruit" improvements to enhance the operational maturity, debuggability, and reliability of the Obscura Server.

## 1. Request ID Tracing
**Goal:** Ensure every log line can be traced back to a specific request ID, enabling debugging in high-concurrency environments.
**Implementation:**
-   **Middleware:** Add `tower_http::request_id` to `src/api/mod.rs`.
-   **Tracing:** Configure `TraceLayer` to inject the request ID into the span.
-   **Behavior:** Use incoming `X-Request-ID` header if present (from Ingress); otherwise generate a UUID.

## 2. Configurable Logging Format
**Goal:** Support both human-readable development logs and machine-parseable production logs.
**Implementation:**
-   **Config:** Add `log_format` enum to `src/config.rs` (Values: `text` (default), `json`).
-   **Env Var:** `OBSCURA_LOG_FORMAT`.
-   **Main:** Update `src/main.rs` to initialize `tracing_subscriber` with the correct formatter based on config.

## 3. Graceful WebSocket Shutdown
**Goal:** Provide a clean disconnection experience for clients during server rollouts.
**Implementation:**
-   **Signal:** Propagate the shutdown signal from `main.rs` to `gateway.rs`.
-   **Handler:** Update `websocket_handler` loop to select on the shutdown signal.
-   **Action:** Send `CloseFrame::GoingAway` and terminate the connection gracefully.

## 4. Password Strength Enforcement
**Goal:** Protect the integrity of the "Encrypted Backup" feature (which relies on password-derived keys).
**Implementation:**
-   **Service:** In `src/core/account_service.rs` (method: `register`).
-   **Rule:** Enforce `password.len() >= 12`.
-   **Error:** Return `AppError::BadRequest` with a clear message if validation fails.

## 5. Structured Panic Hook
**Goal:** Ensure panic details are captured by the logging system (and thus JSON-formatted if enabled) instead of lost to stderr.
**Implementation:**
-   **Main:** Register `std::panic::set_hook` at the start of `main.rs`.
-   **Logic:** Capture panic info (payload + location) and log via `tracing::error!`.
