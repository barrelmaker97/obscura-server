# Observability Gaps & Pending Tasks

## 1. Missing State Metrics
The following metrics are planned but currently have no implementation. These require a background poller to execute aggregate SQL queries.

| Metric Name | Type | Description |
| :--- | :--- | :--- |
| `obscura_users_total` | Gauge | Total number of registered users in the database. |
| `obscura_pending_messages_total` | Gauge | Total number of messages currently waiting for delivery in the `messages` table. |
| `obscura_attachments_total_count` | Gauge | Total number of non-expired attachments. |
| `obscura_attachments_total_bytes` | Gauge | Sum of `content_length` for all stored attachments (may require adding size metadata to the DB). |
| `obscura_db_pool_active_connections` | Gauge | Current number of active connections in the SQLx pool. |

## 2. Advanced Instrumentation

### 2.1 State Metric Poller
Implement a background service (e.g., `MetricPollerWorker`) that runs every 5–10 minutes.
- **Why**: To record growth curves and system inventory without hammering the database on every dashboard refresh.
- **Implementation**: 
    - Executes `SELECT COUNT(*)` for users, messages, and attachments.
    - Records results into OTel Gauges.

### 2.2 RED Metrics (Request, Error, Duration)
Implement Axum middleware to capture standard HTTP telemetry.
- **Metrics**: 
    - `http_server_requests_total` (Counter with `method`, `route`, `status`).
    - `http_server_request_duration_seconds` (Histogram with `route`).
- **Why**: Currently, we have tracing for requests, but no high-level aggregations of API performance or error rates per endpoint.

## 3. Semantic Enrichment

### 3.1 Error Classification
Refine how errors are recorded in traces.
- **Task**: Update `AppError` and the telemetry layer to use OTel Semantic Conventions for exceptions.
- **Goal**: Ensure that a "400 Bad Request" is correctly categorized differently from a "500 Internal Server Error" at the span level, allowing for automated error-rate alerting in tools like Grafana.

## 4. Documentation Debt
- Create a document detailing the metrics found throughout the code (e.g., Push Worker successes/failures, Auth events, and detailed Notification throughput).
