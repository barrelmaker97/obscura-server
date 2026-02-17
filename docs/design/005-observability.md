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
-   **Local Development:** We use the `grafana/otel-lgtm` all-in-one image to simplify the local observability stack.
-   **Behavior:**
    -   If set: Telemetry is fully initialized (Traces + Metrics pushed).
    -   If unset: Runs in "Logs Only" mode (safe default for local dev/testing).
-   **Log Format:** `OBSCURA_LOG_FORMAT=json` for production (links logs to traces via `trace_id`), `text` for dev.

## 3. Metrics Catalog

All metrics use the `obscura_` prefix and follow the pattern: `obscura_[noun]_[unit]_[type]`.

### 3.1 Business Metrics (Product Health)

| Metric Name | Type | Labels | Description | Source | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `obscura_messages_sent_total` | Counter | `status` | Total messages processed. | Event | **Implemented** |
| `obscura_attachment_upload_bytes_total` | Counter | - | Total volume of data successfully uploaded. | Event | **Implemented** |
| `obscura_websocket_connections` | UpDownCounter | - | Active clients. | Event | **Implemented** |
| `obscura_users_total` | Gauge | - | Total registered users. | DB Poller | *Planned* |
| `obscura_registrations_total` | Counter | - | Rate of new signups. | Event | **Implemented** |
| `obscura_pending_messages_total` | Gauge | - | Messages waiting for delivery. | DB Poller | *Planned* |
| `obscura_attachments_total_count` | Gauge | - | Total non-expired attachments. | DB Poller | *Planned* |
| `obscura_attachments_total_bytes` | Gauge | - | Total size of stored data. | DB Poller | *Planned* |
| `obscura_key_takeovers_total` | Counter | - | Security incidents. | Event | **Implemented** |

### 3.2 Operational Metrics (Operational Health)

| Metric Name | Type | Labels | Description | Source | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `obscura_attachment_upload_size_bytes` | Histogram | - | Distribution of upload sizes. | `AttachmentService` | **Implemented** |
| `obscura_message_fetch_batch_size` | Histogram | - | Efficiency of message polling. | `MessageService` | **Implemented** |
| `obscura_rate_limit_decisions_total` | Counter | `status` | Throttling events. | `RateLimit` | **Implemented** |
| `obscura_health_status` | Gauge | `component` | Binary status (1=OK, 0=Error). | `HealthService` | **Implemented** |
| `obscura_db_pool_active_connections` | Gauge | - | SQLx pool saturation. | `sqlx` (Future) | *Planned* |
| `obscura_websocket_outbound_dropped_total` | Counter | `reason` | Drops due to backpressure. | `Gateway` | **Implemented** |
| `obscura_websocket_ack_dropped_total` | Counter | - | ACKs dropped due to backpressure. | `Gateway` | **Implemented** |
| `obscura_notification_worker_processed_total` | Counter | - | Throughput of the real-time dispatcher. | `NotificationWorker` | **Implemented** |

### 3.3 State vs. Activity Metrics (The "DB-Backed" Strategy)

We distinguish between **Activity** (what is happening now) and **State** (the current inventory of the system). For state metrics like `users_total`, we choose **Option A: Application-Level Polling** over direct SQL data sources in Grafana.

**Why Polling?**
- **Time-Series History:** Prometheus records growth curves that DBs usually don't track natively.
- **Unified Alerting:** Alerts for "DB growth" use the same syntax as "CPU spikes".
- **Performance:** Protects the production DB from heavy dashboard-driven aggregate queries.

**Implementation:** A background "Metric Poller" task will run aggregate SQL (e.g., `SELECT COUNT(*)`) every 5-10 minutes and record the result into OTel Gauges.

### 3.4 Tuning & Optimization Metrics (Config Feedback Loop)

| Metric Name | Type | Related Config | Why measure it? | Status |
| :--- | :--- | :--- | :--- | :--- |
| `obscura_websocket_ack_batch_size` | Histogram | `WsConfig.ack_batch_size` | If batches are small, interval is too short. | **Implemented** |
| `obscura_prekey_threshold_reached_total` | Counter | `MessagingConfig.pre_key_refill_threshold` | Frequent low events mean threshold is high. | **Implemented** |
| `obscura_messages_overflow_total` | Counter | `MessagingConfig.max_inbox_size` | Global inbox overflow count. | **Implemented** |
| `obscura_notification_channels` | UpDownCounter | - | Local memory channel pressure. | **Implemented** |

## 4. Tracing Strategy

### 4.1 Philosophy
-   **Verb-Noun Pattern:** All span names MUST follow the `verb_noun` pattern (e.g., `boot_server`, `process_push_jobs`).
-   **Context Envelopes:** Background workers wrap every loop iteration in a span to provide context for child repository/service calls.
-   **Privacy First:** Do NOT trace PII (IP addresses, User Agents).
-   **Correlation:** Every log line in JSON mode MUST include `trace_id` and `span_id`.
-   **Structure:** Follow OTel Semantic Conventions (`http.request.method`, `url.path`, `otel.kind`).

### 4.2 Key Spans
-   **`boot_server`**: The top-level process startup.
-   **`initialize_application`**: Construction of the App/Services/Workers graph.
-   **`websocket_session`**: Lifecycle of a single client connection.
-   **`dispatch_notification`**: Single event moving from Redis to a WebSocket.
-   **`process_push_jobs`**: Trace of a background push notification batch.

## 5. Implementation Roadmap

1.  **Phase 1: Plumbing (DONE)**
    -   OTel crates and `telemetry.rs` initialization.
2.  **Phase 2: Core Metrics & Tracing (DONE)**
    -   Instrumentation of all core services and background cleanup tasks.
3.  **Phase 3: Standards Alignment (DONE)**
    -   Metric naming updated to Prometheus Prometheus standards (`obscura_` + `_total`).
    -   Span naming updated to `verb_noun`.
    -   Workers isolated into dedicated structs for clean tracing context.
4.  **Phase 4: Advanced Instrumentation (PLANNED)**
    -   **State Metric Poller:** Background service for DB-backed gauges.
    -   **RED Metrics:** Axum middleware for standard HTTP metrics.
5.  **Phase 5: Semantic Enrichment (PLANNED)**
    -   **Error Classification:** Use OTel semantic conventions to categorize errors.
