# Obscura Server Roadmap

This document serves as the master index for planned work and architectural vision. Detailed specifications for major features are located in their respective design documents.

## 🗺️ Feature Roadmap

### 📊 Phase 1: Operational Maturity
*Establishing a stable, measurable foundation before adding complexity.*
- [x] **[006: Operational Hardening](006-operational-hardening.md)**: Request tracing, structured logging, and graceful shutdowns. (DONE)
- [x] **[005: Observability](005-observability.md)**: OpenTelemetry-native pipeline for traces, metrics, and business KPIs. (DONE)

### 📱 Phase 2: Mobile Essentials
*Features required for a functional mobile application experience.*
- [ ] **[004: Push Notifications](004-push-notifications.md)**: Generic FCM/APNs alerts for offline delivery and "wake-up" functionality.
- [ ] **[001: Encrypted Cloud Backup](001-encrypted-backup.md)**: Secure account recovery via client-side password encryption to prevent data loss.

### 👤 Phase 3: Identity & Social
*Enhancing the user experience and social features.*
- [ ] **[002: Profile Management](002-profile-management.md)**: Persistent avatars and encrypted profile metadata.

### 🛡️ Phase 4: Privacy Evolution
*Advanced cryptographic privacy features.*
- [ ] **[003: Sealed Sender](003-sealed-sender.md)**: Transitioning to Unidentified Delivery and capability-based blocking.

---

## 🛠️ Continuous Improvement

### 🧪 Quality Assurance & Security
- [ ] **Agent-Based Red Teaming**: Use AI agents to fuzz the API and Protobuf definitions for crashes or vulnerabilities.
- [ ] **Fuzz Testing**: Implement `cargo-fuzz` targets for crypto primitives and protocol decoding.
- [ ] **Load Testing**: Simulate peak WebSocket concurrency using `k6` or `locust`.

### 🧹 Refactoring & Maintenance
- [x] **Error Audit**: Standardize `AppError` mappings across all modules to ensure consistent status codes and zero leakage of sensitive internal details.
- [ ] **Configuration Improvements**: Evaluate the need for dynamic config reloading (SIGHUP) vs. standard container restarts.
- [ ] **CI/CD Enhancements**: Expand the current GitHub Actions to include performance regression checks.
