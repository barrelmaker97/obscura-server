# Plan: Key Management & Signature Verification Hardening

This plan addresses the issues identified during the cryptographic audit of the Obscura server, focusing on performance optimization and replay protection.

## 1. Optimization: Remove Redundant Signature Verification
Currently, `KeyService::upload_keys` verifies the signature of the `SignedPreKey` once before the database transaction and then again inside `upload_keys_internal`.

### Tasks
- [ ] Refactor `KeyService::upload_keys` in `src/core/key_service.rs` to remove the initial `verify_keys` call.
- [ ] Ensure that `upload_keys_internal` remains the single point of truth for signature validation.

## 2. Security: Signed Pre-Key Replay Protection
The server currently allows any validly signed pre-key to be uploaded. To prevent "replay attacks" where an old, potentially compromised key is re-uploaded, the server should enforce monotonic ID increments.

### Tasks
- [ ] Modify `src/storage/key_repo.rs` to include a check for the current `key_id`.
- [ ] Update `KeyService::upload_keys_internal` to enforce that a new `SignedPreKey` must have a `key_id` strictly greater than the one currently stored (unless a `is_takeover` event occurred).
- [ ] Add unit tests in `src/core/key_service.rs` or integration tests to verify that uploading a lower or equal `key_id` fails.

## 3. Security: OTPK Integrity Audit
While One-Time Pre-Keys (OTPKs) are not signed, the server should ensure a single upload doesn't contain duplicate IDs which could lead to unexpected bundle exhaustion.

### Tasks
- [ ] Add a check in `KeyService::upload_keys_internal` to verify that the provided `one_time_pre_keys` batch contains unique IDs.
- [ ] Confirm `KeyRepository::insert_one_time_pre_keys` correctly handles existing keys using its `ON CONFLICT DO NOTHING` clause.

## 4. Verification
- [ ] Run `cargo test` to ensure no regressions in the Signal Protocol flow.
- [ ] Execute `tests/integration_key_formats.rs` and `tests/integration_key_limits.rs` to validate that protocol compatibility is maintained.
