# diesel-clickhouse

Diesel query-builder extensions for ClickHouse SQL.

This crate currently provides a lightweight `ClickHouse` backend for rendering Diesel ASTs as ClickHouse SQL, plus typed helpers for common ClickHouse functions and clauses. It does **not** yet provide a Diesel `Connection` implementation.

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
- SQL type markers: unsigned/wide integers, decimals, enums, tuples/nested, arrays, maps, low-cardinality, JSON, UUID, IPv4/IPv6, BFloat16, AggregateFunction states, DateTime64
- Function bindings via Diesel macros/custom fragments: `toStartOf*`, `toDateTime*`, `dateDiff`, `dateTrunc`, broad type conversions/`CAST`/`accurateCast*`, string/numeric helpers, URL/IP/encoding/hash helpers, vector distance and binary-reference helpers, lambda-based array/map helpers, `if`, `countIf`, `sumIf`, `avgIf`, `minIf`, `maxIf`, aggregate state/merge combinators, `uniq*`, `groupArray*`, `any*`, `argMax`, `argMin`, array/map/JSON path helpers
- Parametric/statistical aggregate fragments: `quantile*`, `quantiles*`, `quantileDeterministic`, `topK`, `histogram`, `corr`, `covar*`
- Grouping modifiers: `WITH TOTALS`, `ROLLUP`, `CUBE`, `GROUPING SETS`, `GROUP BY ALL`, `GROUPING()`
- Query wrappers for scalar `WITH` aliases, CTEs, `QUALIFY`, named `WINDOW`, `LIMIT BY`, `LIMIT ... WITH TIES`, `ORDER BY ... WITH FILL`, `SETTINGS`, `FORMAT`, `INTO OUTFILE`
- Source wrappers for `FINAL`, `SAMPLE`, `SAMPLE ... OFFSET`, `PREWHERE`, `ARRAY JOIN`, `LEFT ARRAY JOIN`, ClickHouse `GLOBAL`/`ANY`/`ALL`/`ASOF`/`SEMI`/`ANTI` joins
- Window helpers: `row_number`, `rank`, `dense_rank`, `lag`, `lead`, `first_value`, `last_value`, `.over(...)`, `.over_window(...)`, `ROWS`/`RANGE` frame builders
- DDL builders for `CREATE TABLE`, MergeTree-family/special engines, projections, vector similarity indexes, materialized views, and common `ALTER TABLE` operations
- `GLOBAL IN` / `GLOBAL NOT IN` operators

See `tests/sql_render.rs` for render examples and `docs/FEATURE_MATRIX.md` for the implementation checklist.

## Live ClickHouse tests

The integration battery in `tests/live_clickhouse.rs` starts a real `clickhouse/clickhouse-server` container with `testcontainers`, creates a scratch `ReplacingMergeTree` table, executes SQL rendered by this crate through the official `clickhouse` Rust client, and lets testcontainers tear the container down when the test exits.

It is ignored by default so ordinary `cargo test` does not require Docker:

```bash
cargo test --test live_clickhouse -- --ignored --nocapture
```
