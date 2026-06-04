# diesel-clickhouse

[![Crates.io](https://img.shields.io/crates/v/diesel-clickhouse.svg)](https://crates.io/crates/diesel-clickhouse)
[![docs.rs](https://docs.rs/diesel-clickhouse/badge.svg)](https://docs.rs/diesel-clickhouse)

[Diesel](https://diesel.rs/) query-builder extensions for [ClickHouse](https://clickhouse.com/) SQL.

This crate provides a lightweight `ClickHouse` backend for rendering Diesel ASTs as ClickHouse SQL, typed helpers for common ClickHouse functions and clauses, and an HTTP-backed native async Diesel connection for idiomatic `load`/`execute` workflows.

```rust,ignore
use diesel::prelude::*;
use diesel_clickhouse::{count_if, quantile, to_sql, ClickHouseQueryDsl, Format};

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    events (tenant_id) {
        tenant_id -> Text,
        success -> Bool,
        latency_ms -> Double,
    }
}

let query = events::table
    .filter(events::tenant_id.eq("acme"))
    .group_by(events::tenant_id)
    .select((
        events::tenant_id,
        count_if(events::success),
        quantile(0.95, events::latency_ms),
    ))
    .limit_by_col(10, "tenant_id")
    .format(Format::JsonEachRow);

let sql = to_sql(&query)?;
```

## Supported Features

See the [Feature Matrix](https://docs.rs/diesel-clickhouse/latest/diesel_clickhouse/docs/feature_matrix/index.html) for the currently supported and planned features.

See `docs/USAGE.md` for usage guidance, `docs/TUTORIAL.md` for a ClickHouse NYC taxi tutorial translated to Diesel, `tests/sql_render.rs` for render examples, and `docs/CONNECTION_DESIGN.md` for connection design notes.

## Installation

```toml
[dependencies]
diesel-clickhouse = "0.6"
diesel = { version = "2.3", default-features = false }
```

Enable native BigDecimal decimal values when needed:

```toml
[dependencies]
diesel-clickhouse = { version = "0.6", features = ["bigdecimal"] }
```

Note that this crate re-exports `clickhouse` so you can use `diesel_clickhouse::clickhouse` to access the underlying client for features outside the scope of Diesel.
We do not re-export `diesel` itself, so you have to verify that your `diesel` version matches.
We are currently supporting Diesel (>=2.3, <2.4). 

## Tutorial

The NYC taxi tutorial in `docs/TUTORIAL.md` shows ClickHouse SQL alongside equivalent Diesel code. It has an executable companion that can run the tutorial against a disposable ClickHouse container and write a Markdown report with observed results:

```bash
just tutorial
```

If you already have ClickHouse running, call the example directly:

```bash
CLICKHOUSE_URL=http://default:password@localhost:8123/default \
  cargo run --example tutorial -- --write docs/TUTORIAL.md
```

## Live ClickHouse tests

The integration battery in `tests/live_clickhouse.rs` starts a real `clickhouse/clickhouse-server` container with `testcontainers`, creates scratch tables, executes SQL rendered by this crate through the official `clickhouse` Rust client, verifies the native async `AsyncClickHouseConnection` against the same live server, and lets testcontainers tear the container down when the test exits.

It is ignored by default so ordinary `cargo test` does not require Docker:

```bash
cargo test --test live_clickhouse -- --ignored --nocapture
```

The repo also ships a `justfile` for local validation:

```bash
just ci
```

That runs default and `bigdecimal` tests, live ClickHouse tests, and clippy.

## Releasing

Releases are cut with [`cargo release`](https://github.com/crate-ci/cargo-release). The cargo-release configuration updates the changelog, creates a release commit and `vX.Y.Z` tag, and pushes them. Publishing is handled by the GitHub release workflow when that tag is pushed.

For the initial `0.1.0` release, the version is already set:

```bash
cargo release --execute
```

For later releases, pass the semver bump or explicit version:

```bash
cargo release patch --execute   # or: minor / major / 0.2.0
```

The release workflow expects a `CARGO_REGISTRY_TOKEN` repository secret.

## License

Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
