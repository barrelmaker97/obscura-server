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
[Obscura Server]  --(OTLP/HTTP)-->  [OTel Collector]  --> [Prometheus / Mimir] (Metrics)
                                                      --> [Tempo / Jaeger]     (Traces)
                                                      --> [Loki]               (Logs - via Promtail)
```

### 2.2 Configuration
-   **Env Var:** `OBSCURA_OTLP_ENDPOINT` (e.g., `http://otel-collector:4318`).
-   **Behavior:**
    -   If set: Telemetry is fully initialized (Traces + Metrics pushed).
    -   If unset: Runs in "Logs Only" mode (safe default for local dev/testing).
-   **Log Format:** `OBSCURA_LOG_FORMAT=json` for production (links logs to traces via `trace_id`), `text` for dev.

## 3. Metrics Catalog

We distinguish between **Application Metrics** (Is the engine running?) and **Business Metrics** (Is the product working?).

### 3.1 Business Metrics (Product Health)

| Metric Name | Type | Labels | Description | Service |
| :--- | :--- | :--- | :--- | :--- |
| `messages_sent_total` | Counter | `status=success\|failure` | Total messages successfully processed. Primary growth KPI. | `MessageService` |
| `attachments_uploaded_bytes` | Counter | - | Total volume of data stored. Primary cost driver. | `AttachmentService` |
| `websocket_active_connections` | Gauge | - | Number of currently connected clients. Engagement KPI. | `Gateway` |
| `websocket_session_duration_seconds` | Histogram | - | How long users stay connected. Indicates client stability. | `Gateway` |
| `keys_takeover_events_total` | Counter | - | Number of device takeovers. Spike = Security Incident. | `KeyService` |
| `users_registered_total` | Counter | `status` | New user signups. Growth KPI. | `AccountService` |

### 3.2 Application Metrics (Operational Health)

| Metric Name | Type | Labels | Description | Source |
| :--- | :--- | :--- | :--- | :--- |
| `http_requests_total` | Counter | `method`, `route`, `status` | Volume and Error Rate. | `TraceLayer` (Middleware) |
| `http_request_duration_seconds` | Histogram | `method`, `route` | Latency distribution (P95, P99). | `TraceLayer` (Middleware) |
| `db_pool_active_connections` | Gauge | - | Database saturation. | `sqlx` (Future) |
| `rate_limit_hits_total` | Counter | `route`, `source_type` | Throttling events. Indicates abuse or capacity limits. | `RateLimit` Middleware |

### 3.3 Tuning & Optimization Metrics (Config Feedback Loop)

These metrics exist specifically to help tune `src/config.rs` parameters.

| Metric Name | Type | Related Config | Why measure it? |
| :--- | :--- | :--- | :--- |
| `rate_limit_decisions_total` | Counter (`status=throttled\|allowed`) | `RateLimitConfig.per_second` | If legitimate users are throttled, limits are too tight. |
| `websocket_ack_batch_size` | Histogram | `WsConfig.ack_batch_size`, `ack_flush_interval_ms` | If batches are small, interval is too short. If full, interval is too long. |
| `keys_prekey_low_events_total` | Counter | `MessagingConfig.pre_key_refill_threshold` | Frequent low events mean the threshold is too high or clients are buggy. |
| `notification_channel_full_total` | Counter | `NotificationConfig.channel_capacity` | Drops here mean the capacity (16) is too small for the burst rate. |
| `health_check_duration_seconds` | Histogram | `HealthConfig.db_timeout_ms` | If duration nears timeout, the check is too aggressive or DB is slow. |

## 4. Tracing Strategy

### 4.1 Philosophy
-   **Privacy First:** Do NOT trace PII (IP addresses, User Agents) to avoid building a permanent location history.
-   **Correlation:** Every log line in JSON mode MUST include `trace_id` and `span_id`.
-   **Structure:** Follow OTel Semantic Conventions (`http.request.method`, `url.path`, `otel.kind`).

### 4.2 Key Spans
-   **Root Span:** HTTP Request or WebSocket Session.
-   **Service Layer:** `#[instrument(err)]` on `MessageService`, `KeyService`.
-   **Repository Layer:** Database queries (Debug level to avoid noise).

## 5. Implementation Roadmap

1.  **Phase 1: Plumbing (DONE)**
    -   Add OTel crates (`opentelemetry`, `tracing-opentelemetry`).
    -   Implement `telemetry.rs` initialization logic.
    -   Make OTLP endpoint configurable.

2.  **Phase 2: Business Metrics (NEXT)**
    -   Instrument `MessageService` (`messages_sent_total`).
    -   Instrument `AttachmentService` (`attachments_uploaded_bytes`).
    -   Instrument `Gateway` (`websocket_active_connections`).

3.  **Phase 3: Infrastructure**
    -   Deploy LGTM Stack (Loki, Grafana, Tempo, Mimir) or Jaeger/Prometheus via Helm.
    -   Update `docker-compose.yml` for local testing.