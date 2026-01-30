# Design Doc 005: Observability & Metrics

## 1. Overview
We need visibility into the runtime health and performance of the server. This document covers **Application Metrics** exposed via a Prometheus-compatible `/metrics` endpoint.

### 1.1 Security
-   The `/metrics` endpoint **MUST** be exposed **ONLY on the Management Port** (default: 9090).
-   It **MUST NOT** be accessible via the public API port (3000) or the public internet.
-   This prevents leakage of business intelligence (user counts, traffic patterns) to attackers.

## 2. Technology Stack
-   **Crate:** `metrics` (facade)
-   **Exporter:** `metrics-exporter-prometheus` (axum integration)

## 3. Metrics to Instrument

### 3.1 HTTP Layer (Middleware)
-   `http_requests_total` (Counter): Labels: `method`, `route`, `status`.
-   `http_request_duration_seconds` (Histogram): Latency distribution.

### 3.2 WebSocket / Gateway
-   `active_connections` (Gauge): Current number of connected clients.
-   `ws_messages_sent_total` (Counter): Outbound traffic.
-   `ws_messages_received_total` (Counter): Inbound traffic (ACKs, etc).

### 3.3 Internal Queues (Backpressure Detection)
Crucial for diagnosing performance bottlenecks.
-   `channel_queue_depth` (Gauge): Labels: `type="outbound"`, `type="ack"`.
-   `channel_dropped_messages` (Counter): Messages dropped due to full buffers.

### 3.4 Infrastructure Latency
-   **Database**:
    -   `db_pool_wait_duration_seconds` (Histogram): Time spent waiting for a connection.
    -   `db_query_duration_seconds` (Histogram): Time spent executing queries.
-   **S3 (Attachments)**:
    -   `s3_upload_duration_seconds` (Histogram).
    -   `s3_download_duration_seconds` (Histogram).

## 4. Implementation Plan
1.  **Setup**: Initialize `PrometheusBuilder` in `main.rs` and attach the handle to the `/metrics` route in `health.rs`.
2.  **Middleware**: Create a `TraceLayer` or custom middleware to record HTTP metrics.
3.  **DB Wrapper**: Add a transparent wrapper or interceptor around `db.acquire()` to measure pool wait times.
4.  **Channel Instrumentation**: Update `gateway.rs` to emit metrics when pushing/popping from `mpsc` channels.
