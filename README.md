# diesel-clickhouse

Diesel query-builder extensions for ClickHouse SQL.

This crate provides a lightweight `ClickHouse` backend for rendering Diesel ASTs as ClickHouse SQL, typed helpers for common ClickHouse functions and clauses, and an initial HTTP-backed Diesel `Connection` implementation for idiomatic `load`/`execute` workflows.

```rust,ignore
use diesel::prelude::*;
use diesel_clickhouse::{count_if, quantile, to_sql, ClickHouseQueryDsl, Format};

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

## Implemented so far

- `ClickHouse` backend marker and query builder (`?` binds, backtick identifiers)
- Initial `ClickHouseConnection` over ClickHouse HTTP for Diesel `load`, `first`, `execute`, `batch_execute`, explicit `ClickHouseConnectionOptions`, server-side HTTP parameters for supported binds, primitive/text/nullable rows, arrays into `Vec<T>`, maps into `BTreeMap<K, V>`, tuples into Rust tuples, string-form Decimal/Date/DateTime/UUID/IP/JSON/Dynamic/Variant values, optional `bigdecimal::BigDecimal` decimal support, and `sql_query`; transactions intentionally return an unsupported error
- SQL type markers: unsigned/wide integers, decimals, enums, tuples/nested, arrays, maps, low-cardinality, JSON, UUID, IPv4/IPv6, Point/Ring, Dynamic/Variant, BFloat16, AggregateFunction states, DateTime64
- Function bindings via Diesel macros/custom fragments: `toStartOf*`, `toDateTime*`, `dateDiff`, `dateTrunc`, broad type conversions/`CAST`/`accurateCast*`, string/numeric/search helpers (`LIKE`/`ILIKE`, `match`, `multiMatch*`), URL/IP/encoding/hash helpers, vector distance and binary-reference helpers, lambda-based array/map helpers, `if`, `countIf`, `sumIf`, `avgIf`, `minIf`, `maxIf`, generic aggregate combinator builder, aggregate state/merge combinators, `uniq*`, `groupArray*`, `any*`, `argMax`, `argMin`, statistical aggregates (`stddev*`, `var*`, ANOVA, Mann-Whitney, `approx_top_sum`), array/map/JSON path helpers
- Parametric/statistical aggregate fragments: `quantile*`, `quantiles*`, `quantileDeterministic`, `topK`, `histogram`, `corr`, `covar*`
- Grouping modifiers: `WITH TOTALS`, `ROLLUP`, `CUBE`, `GROUPING SETS`, `GROUP BY ALL`, `GROUPING()`
- Diesel-native query clauses covered for ClickHouse rendering: `WHERE`, `HAVING`, `ORDER BY`, `GROUP BY`, `LIMIT`/`OFFSET`, nullable predicates, and comparison/logical operators
- Query wrappers for scalar `WITH` aliases, CTEs, `QUALIFY`, named `WINDOW`, `LIMIT BY`, `LIMIT ... WITH TIES`, `ORDER BY ... WITH FILL`, `SETTINGS`, `FORMAT`, `INTO OUTFILE`
- Source wrappers for `FINAL`, `SAMPLE`, `SAMPLE ... OFFSET`, `PREWHERE`, `ARRAY JOIN`, `LEFT ARRAY JOIN`, ClickHouse `GLOBAL`/`ANY`/`ALL`/`ASOF`/`SEMI`/`ANTI` joins; use these join wrappers for executable ClickHouse join SQL rather than Diesel's parenthesized built-in join source rendering
- Window helpers: `row_number`, `rank`, `dense_rank`, `lag`, `lead`, `first_value`, `last_value`, `.over(...)`, `.over_window(...)`, `ROWS`/`RANGE` frame builders
- DDL builders for `CREATE TABLE`, MergeTree-family/special engines, projections, vector similarity indexes, materialized views, and broad `ALTER TABLE` operations including mutations and partitions
- `GLOBAL IN` / `GLOBAL NOT IN` operators

See `docs/USAGE.md` for usage guidance, `docs/TUTORIAL.md` for a ClickHouse NYC taxi tutorial translated to Diesel, `tests/sql_render.rs` for render examples, `docs/FEATURE_MATRIX.md` for the implementation checklist, and `docs/CONNECTION_DESIGN.md` for connection design notes.

## Installation

```toml
[dependencies]
diesel-clickhouse = "0.1"
diesel = { version = "2.2", default-features = false }
```

Enable native BigDecimal decimal values when needed:

```toml
[dependencies]
diesel-clickhouse = { version = "0.1", features = ["bigdecimal"] }
```

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

The integration battery in `tests/live_clickhouse.rs` starts a real `clickhouse/clickhouse-server` container with `testcontainers`, creates scratch tables, executes SQL rendered by this crate through the official `clickhouse` Rust client, verifies `ClickHouseConnection` against the same live server, and lets testcontainers tear the container down when the test exits.

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

Releases are cut with [`cargo release`](https://github.com/crate-ci/cargo-release). The cargo-release configuration bumps the version and pushes a `vX.Y.Z` tag; the GitHub release workflow publishes that tag to crates.io.

```bash
cargo release patch --execute   # or: minor / major
```

The release workflow expects a `CARGO_REGISTRY_TOKEN` repository secret.

## License

Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
