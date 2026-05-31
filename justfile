# Run `just` with no arguments to see the available recipes.

# List available recipes.
default:
    @just --list

# Full local validation, including the Docker-backed ClickHouse integration test.
test: test-unit test-examples test-live clippy

# Fast checks that do not require Docker.
test-unit:
    cargo test

# Compile and test executable examples, including the tutorial generator.
test-examples:
    cargo test --examples

# Fast checks with the optional BigDecimal feature enabled.
test-bigdecimal:
    cargo test --features bigdecimal

# Live integration tests. The test harness starts ClickHouse with testcontainers.
test-live:
    cargo test --test live_clickhouse -- --ignored --nocapture

# Live integration tests with optional BigDecimal coverage enabled.
test-live-bigdecimal:
    cargo test --features bigdecimal --test live_clickhouse -- --ignored --nocapture

# Lint all targets with the default feature set.
clippy:
    cargo clippy --all-targets -- -D warnings

# Lint all targets with optional BigDecimal support enabled.
clippy-bigdecimal:
    cargo clippy --all-targets --features bigdecimal -- -D warnings

# Everything CI should care about before cutting a release.
ci: test-unit test-examples test-bigdecimal test-live test-live-bigdecimal clippy clippy-bigdecimal
