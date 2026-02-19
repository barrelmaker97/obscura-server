# 🛠️ Miscellaneous

## 🧪 Quality Assurance & Security
- [ ] **Agent-Based Red Teaming**: Use AI agents to fuzz the API and Protobuf definitions for crashes or vulnerabilities.
- [ ] **Fuzz Testing**: Implement `cargo-fuzz` targets for crypto primitives and protocol decoding.
- [ ] **Load Testing**: Simulate peak WebSocket concurrency using `k6` or `locust`.

## 🧹 Refactoring & Maintenance
- [x] **Error Audit**: Standardize `AppError` mappings across all modules to ensure consistent status codes and zero leakage of sensitive internal details.
- [ ] **Configuration Improvements**: Evaluate the need for dynamic config reloading (SIGHUP) vs. standard container restarts.
- [ ] **CI/CD Enhancements**: Expand the current GitHub Actions to include performance regression checks.
