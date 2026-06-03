# Changelog

All notable changes to `diesel-clickhouse` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

The crate's major version tracks Diesel's third-party backend surface: a Diesel
3.x release should correspond to a `diesel-clickhouse` 3.x release.

## [Unreleased]

## [0.6.0] — 2026-06-03

### Changed
- **Breaking:** the connection is now native async. `ClickHouseConnection` is replaced by `AsyncClickHouseConnection`, which implements [`diesel_async::AsyncConnection`] directly over the async `clickhouse` client's futures — no owned Tokio runtime and no `block_on`. Drive queries with `diesel_async::RunQueryDsl` and `.await` (e.g. `query.load(&mut conn).await?`); `establish`, `ClickHouseConnectionOptions::connect`, `insert_batch`, and `batch_execute` are now `async`. Callers no longer need `spawn_blocking`/`r2d2`, and the "Cannot start a runtime from within a runtime" hazard is gone.
  - Import `diesel_async::RunQueryDsl` explicitly; it shadows the `RunQueryDsl` from diesel's prelude glob so method resolution picks the async connection's methods.
  - Sync/blocking callers (and `diesel_migrations`) can wrap it in diesel-async's `AsyncConnectionWrapper` via the new `async-connection-wrapper` feature.

### Added
- `diesel-async` 0.7 dependency and native `AsyncConnection`/`AsyncConnectionCore`/`SimpleAsyncConnection` impls for ClickHouse.
- Opt-in connection pooling behind the `bb8`, `deadpool`, and `mobc` features (each enables the matching diesel-async pool integration plus the `PoolableConnection` impl).
- `async-connection-wrapper` feature exposing diesel-async's sync/migration adapter.

### Removed
- The blocking `ClickHouseConnection`, its owned current-thread runtime, and the `ClickHouseCursor` cursor type (the async connection streams rows instead).

## [0.5.0] — 2026-06-03

### Added
- `ClickHouseConnection::insert_batch(table, rows)`: multi-row ingestion through the `clickhouse` client's native RowBinary inserter (one columnar request per batch), returning the number of rows sent. Diesel's DSL still cannot express multi-row `INSERT` on a third-party backend; this is the supported high-throughput write path.

### Changed
- `execute`/`execute_returning_count` now report the number of written rows ClickHouse declares in its `X-ClickHouse-Summary` response trailer instead of always returning `0`. The execute path runs with `wait_end_of_query=1` so the count reflects the completed write. Statements ClickHouse does not count (DDL, some background `ALTER ... DELETE`/`UPDATE` mutations) still report `0`.

### Fixed
- `cargo test --doc` is clean again: the narrative `docs/TUTORIAL.md` snippets are marked `ignore` (they reference schema/bindings defined elsewhere in the guide and were never standalone-compilable), matching the convention already used in `docs/USAGE.md`.

## [0.4.0] — 2026-06-03

### Changed
- Upgraded the `clickhouse` client dependency from `0.13.3` to `0.15.0` and migrated the deprecated `with_option` calls to `with_setting` (behaviour-identical). MSRV unaffected (the client's 1.79 floor is below this crate's 1.96).

### Added
- `join_column(...)` helper: makes a Diesel table column selectable from a `ClickHouseJoin` source while preserving its SQL type, replacing hand-written `sql::<...>("...")` join projections with type-checked select lists.
- Idiomatic single-row inserts through `ClickHouseConnection`: `insert_into(t).values((col.eq(v), ...))` and `#[derive(Insertable)]` structs (with `#[diesel(treat_none_as_default_value = false)]`).

### Changed
- Result loading shares one column-name header (`Arc`) across all rows in a result set instead of cloning a `HashMap` and the column-name strings per row, cutting allocations when loading large results.
- Backend now reports `DoesNotSupportBatchInsert` (was the incorrect `PostgresLikeBatchInsertSupport`). Multi-row batch inserts are not expressible through Diesel on a third-party backend; use the `clickhouse` client's RowBinary `insert()`/`inserter()` for high-throughput ingestion (documented in `docs/USAGE.md`).

### Documentation
- Documented that `ClickHouseConnection` is blocking and must be called from a blocking context (not directly from an `async fn`); `diesel-async` is noted as future work.
- Documented precisely why `execute` returns `0`: ClickHouse reports written rows in `X-ClickHouse-Summary`, but the `clickhouse` client's `execute()` discards that header.

## [0.3.0] — 2026-06-01

### Added
- Fluent `.over_ch(spec)` helper for ClickHouse window specifications, avoiding ambiguity with Diesel 2.3's no-argument `.over()` method.

### Changed
- Require Diesel 2.3 (`>=2.3, <2.4`) and current stable Rust 1.96.

## [0.2.1] — 2026-05-31

## [0.2.0] — 2026-05-31

Initial release.

### Added
- ClickHouse Diesel backend marker and query builder.
- SQL rendering helper via `diesel_clickhouse::to_sql`.
- HTTP-backed `ClickHouseConnection` with `load`, `first`, `execute`, and `batch_execute`.
- Explicit `ClickHouseConnectionOptions` for URL, credentials, database, and ClickHouse HTTP options/settings.
- Server-side HTTP parameters for supported binds, with escaped-literal fallback for ambiguous cases.
- Row decoding for primitives, nullable values, arrays, maps, tuples, string-form date/time/UUID/IP/JSON/Dynamic/Variant values, and optional BigDecimal decimals.
- ClickHouse SQL type markers, DDL builders, query clause extensions, functions, aggregates, vector helpers, joins, windows, grouping extensions, and live ClickHouse coverage.
- NYC taxi tutorial and executable tutorial example.

[Unreleased]: https://github.com/cjrh/diesel-clickhouse/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.6.0
[0.5.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.5.0
[0.4.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.4.0
[0.3.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.3.0
[0.2.1]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.2.1
[0.2.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.2.0
