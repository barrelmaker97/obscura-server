# Design Doc 005: Observability & Metrics

## 1. Overview
We have adopted an **OpenTelemetry-native** observability strategy. Instead of exposing local endpoints for scraping (the legacy Prometheus model), the server **pushes** telemetry data (Traces and Metrics) via the OTLP protocol to a central collector.

This provides:
1.  **Unified Pipeline:** Traces and Metrics travel together.
2.  **Vendor Agnosticism:** We can switch backends (Tempo, Jaeger, Datadog, Honeycomb) by reconfiguring the Collector, not the app.
3.  **Security:** No need to expose management ports or sensitive metrics to the public network or sidecars.

## 2. Architecture

### 2.1 The Pipeline
```
[Obscura Server]  --(OTLP/HTTP)-->  [OTel LGTM Stack] (Loki, Grafana, Tempo, Prometheus)
```

### 2.2 Configuration
-   **Env Var:** `OBSCURA_OTLP_ENDPOINT` (e.g., `http://lgtm:4318`).
-   **Local Development:** We use the `grafana/otel-lgtm` all-in-one image to simplify the local observability stack. This image includes Grafana, Prometheus, Loki, and Tempo with an OTLP-compatible ingestion endpoint.
-   **Behavior:**
    -   If set: Telemetry is fully initialized (Traces + Metrics pushed).
    -   If unset: Runs in "Logs Only" mode (safe default for local dev/testing).
-   **Log Format:** `OBSCURA_LOG_FORMAT=json` for production (links logs to traces via `trace_id`), `text` for dev.

## 3. Metrics Catalog

We distinguish between **Application Metrics** (Is the engine running?) and **Business Metrics** (Is the product working?).

### 3.1 Business Metrics (Product Health)

| Metric Name | Type | Labels | Description | Source | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `messages_sent_total` | Counter | `status` | Total messages processed. | Event | **Implemented** |
| `attachments_uploaded_bytes` | Counter | - | Total volume of data stored. | Event | **Implemented** |
| `websocket_active_connections` | UpDownCounter | - | Active clients. | Event | **Implemented** |
| `users_total` | Gauge | - | Total registered users. | DB Poller | *Planned* |
| `users_registered_total` | Counter | - | Rate of new signups. | Event | **Implemented** |
| `pending_messages_total` | Gauge | - | Messages waiting for delivery. | DB Poller | *Planned* |
| `attachments_total_count` | Gauge | - | Total non-expired attachments. | DB Poller | *Planned* |
| `attachments_total_bytes` | Gauge | - | Total size of stored data. | DB Poller | *Planned* |
| `keys_takeovers_total` | Counter | - | Security incidents. | Event | **Implemented** |

### 3.2 Application Metrics (Operational Health)

| Metric Name | Type | Labels | Description | Source | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `attachments_upload_size_bytes` | Histogram | - | Distribution of upload sizes. | `AttachmentService` | **Implemented** |
| `messaging_fetch_batch_size` | Histogram | - | Efficiency of message polling. | `MessageService` | **Implemented** |
| `rate_limit_decisions_total` | Counter | `status` | Throttling events. | `RateLimit` Middleware | **Implemented** |
| `health_status` | Gauge | `component` | Binary status (1=OK, 0=Error). | `HealthService` | **Implemented** |
| `db_pool_active_connections` | Gauge | - | SQLx pool saturation. | `sqlx` (Future) | *Planned* |
| `websocket_outbound_dropped_total` | Counter | `reason` | Drops due to backpressure. | `Gateway` | **Implemented** |
| `websocket_ack_queue_dropped_total` | Counter | - | ACKs dropped due to backpressure. | `Gateway` | **Implemented** |

### 3.3 State vs. Activity Metrics (The "DB-Backed" Strategy)

We distinguish between **Activity** (what is happening now) and **State** (the current inventory of the system). For state metrics like `users_total`, we choose **Option A: Application-Level Polling** over direct SQL data sources in Grafana.

**Why Polling?**
- **Time-Series History:** Prometheus records growth curves that DBs usually don't track natively.
- **Unified Alerting:** Alerts for "DB growth" use the same syntax as "CPU spikes".
- **Performance:** Protects the production DB from heavy dashboard-driven aggregate queries.

**Implementation:** A background "Metric Poller" task will run aggregate SQL (e.g., `SELECT COUNT(*)`) every 5-10 minutes and record the result into OTel Gauges.

### 3.4 Tuning & Optimization Metrics (Config Feedback Loop)

These metrics exist specifically to help tune `src/config.rs` parameters.

| Metric Name | Type | Related Config | Why measure it? | Status |
| :--- | :--- | :--- | :--- | :--- |
| `websocket_ack_batch_size` | Histogram | `WsConfig.ack_batch_size` | If batches are small, interval is too short. | **Implemented** |
| `keys_prekey_low_events_total` | Counter | `MessagingConfig.pre_key_refill_threshold` | Frequent low events mean threshold is high. | **Implemented** |
| `messaging_inbox_overflow_total` | Counter | `MessagingConfig.max_inbox_size` | Global inbox overflow count. | **Implemented** |
| `notification_sends_total` | Counter | `status` | Internal notification success rate. | **Implemented** |

## 4. Tracing Strategy

### 4.1 Philosophy
-   **Privacy First:** Do NOT trace PII (IP addresses, User Agents) to avoid building a permanent location history.
-   **Correlation:** Every log line in JSON mode MUST include `trace_id` and `span_id`.
-   **Structure:** Follow OTel Semantic Conventions (`http.request.method`, `url.path`, `otel.kind`).

### 4.2 Key Spans
-   **Root Span:** HTTP Request (`request_id` injected) or WebSocket Session (`user_id` and `request_id` injected).
-   **Service Layer:** `#[instrument(err)]` on `MessageService`, `KeyService`, `AccountService`, and `AttachmentService`.
-   **Repository Layer:** Database queries (via `tracing` at debug level).

## 5. Implementation Roadmap

1.  **Phase 1: Plumbing (DONE)**
    -   Add OTel crates (`opentelemetry`, `tracing-opentelemetry`).
    -   Implement `telemetry.rs` initialization logic.
    -   Make OTLP endpoint configurable.

2.  **Phase 2: Core Metrics (DONE)**
    -   Instrument `MessageService`, `AttachmentService`, `KeyService`, and `Gateway`.
    -   Implement `health_status` binary gauge.

3.  **Phase 3: Infrastructure (DONE)**
    -   Update `docker-compose.yml` with `grafana/otel-lgtm` for simplified local testing.

4.  **Phase 4: Advanced Instrumentation (PLANNED)**
    -   **State Metric Poller:** Implement a background service to query and push DB-backed gauges (`users_total`, `pending_messages_total`, etc.).
    -   **RED Metrics:** Implement explicit Axum middleware for `http_requests_total` and `http_request_duration_seconds` using `MatchedPath`.
    -   **Resource Auto-Instrumentation:** Enable `sqlx` (pool metrics) and `aws-sdk` telemetry to capture internal dependency performance automatically.
    -   **Registration Metrics:** Add `users_registered_total` counter to `AccountService` to track signup velocity (DONE).
    -   **Session Metrics:** Add `websocket_session_duration_seconds` to `Gateway`.

5.  **Phase 5: Semantic Enrichment (PLANNED)**
    -   **User Context:** Inject `user_id` into spans after successful JWT authentication (implemented in `AuthUser` middleware and WebSocket handler).
    -   **Error Classification:** Use OTel semantic conventions to categorize errors (e.g., `db.error.condition`).
