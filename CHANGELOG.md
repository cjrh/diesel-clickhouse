# Changelog

All notable changes to `diesel-clickhouse` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

The crate's major version tracks Diesel's third-party backend surface: a Diesel
3.x release should correspond to a `diesel-clickhouse` 3.x release.

## [Unreleased]

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

[Unreleased]: https://github.com/cjrh/diesel-clickhouse/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.3.0
[0.2.1]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.2.1
[0.2.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.2.0
