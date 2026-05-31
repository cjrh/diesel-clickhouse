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

# Run the NYC taxi tutorial against a disposable ClickHouse container.
tutorial output="docs/TUTORIAL.md":
    #!/usr/bin/env bash
    set -euo pipefail

    container="diesel-clickhouse-tutorial"
    image="clickhouse/clickhouse-server:25.3"
    password="password"

    cleanup() {
        docker rm -f "$container" >/dev/null 2>&1 || true
    }
    cleanup
    trap cleanup EXIT

    docker run -d --name "$container" \
        -p 127.0.0.1::8123 \
        -e CLICKHOUSE_PASSWORD="$password" \
        --ulimit nofile=262144:262144 \
        "$image" >/dev/null

    host_port="$(docker port "$container" 8123/tcp | sed 's/.*://')"
    root_url="http://default:${password}@127.0.0.1:${host_port}"
    clickhouse_url="${root_url}/default"

    echo "Waiting for ClickHouse at ${root_url} ..."
    ready=0
    for _ in {1..90}; do
        if curl -fsS "${root_url}/?query=SELECT%201" >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 1
    done

    if [[ "$ready" != 1 ]]; then
        docker logs --tail 80 "$container" >&2 || true
        exit 1
    fi

    CLICKHOUSE_URL="$clickhouse_url" cargo run --example tutorial -- --write '{{output}}'
    echo "Tutorial Markdown written to {{output}}"

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
