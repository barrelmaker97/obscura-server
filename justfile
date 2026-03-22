# List available recipes
default:
    @just --list

# Type-check without building
check:
    cargo check

# Build debug binary
build:
    cargo build

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# Run clippy lints
clippy:
    cargo clippy -- -D warnings

# Run tests
test:
    cargo test

# Run full CI suite locally (format check + clippy + coverage)
ci: fmt-check clippy coverage

# Start backing services for local development
services:
    docker compose up -d db minio valkey
    @echo "Waiting for services..."
    @docker compose exec db sh -c 'until pg_isready -U user -d signal_server; do sleep 1; done' > /dev/null 2>&1
    @until curl -sf http://localhost:9000/minio/health/live > /dev/null 2>&1; do sleep 1; done
    @docker compose exec -e AWS_ACCESS_KEY_ID=minioadmin -e AWS_SECRET_ACCESS_KEY=minioadmin minio sh -c "mkdir -p /data/test-bucket"
    @echo "Services ready."

# Stop backing services
services-down:
    docker compose down

# Generate LCOV coverage report
coverage: _test-coverage
    grcov . \
        --binary-path ./target/debug/ \
        -s . \
        -t lcov \
        --branch \
        --ignore-not-existing \
        --ignore "/*" \
        --ignore "target/*" \
        --ignore "tests/*" \
        --ignore "build.rs" \
        -o lcov.info
    @echo ""
    @echo "Coverage report written to lcov.info"
    @awk '/^LH:/{h+=substr($0,4)} /^LF:/{t+=substr($0,4)} END{printf "Overall: %d/%d lines (%.1f%%)\n",h,t,(h/t)*100}' lcov.info
    find . -name "*.profraw" -delete

# Generate HTML coverage report
coverage-html: _test-coverage
    grcov . \
        --binary-path ./target/debug/ \
        -s . \
        -t html \
        --branch \
        --ignore-not-existing \
        --ignore "/*" \
        --ignore "target/*" \
        --ignore "tests/*" \
        --ignore "build.rs" \
        -o coverage/
    @echo ""
    @echo "HTML report written to coverage/"
    find . -name "*.profraw" -delete

# Remove coverage artifacts
clean-coverage:
    rm -rf lcov.info coverage/
    find . -name "*.profraw" -delete

_test-coverage:
    find . -name "*.profraw" -delete
    CARGO_INCREMENTAL=0 \
    RUSTFLAGS="-C instrument-coverage" \
    LLVM_PROFILE_FILE="obscura-%p-%m.profraw" \
    cargo test
