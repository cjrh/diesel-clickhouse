# Changelog

All notable changes to `diesel-clickhouse` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

The crate's major version tracks Diesel's third-party backend surface: a Diesel
3.x release should correspond to a `diesel-clickhouse` 3.x release.

## [Unreleased]

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

[Unreleased]: https://github.com/cjrh/diesel-clickhouse/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.2.1
[0.2.0]: https://github.com/cjrh/diesel-clickhouse/releases/tag/v0.2.0
