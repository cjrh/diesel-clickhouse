# Diesel `Connection` design notes

The crate now includes an initial HTTP-backed `ClickHouseConnection`. This document records the design constraints, current scope, and remaining work so the connection can evolve without pretending ClickHouse is a regular transactional OLTP backend.

## Current spike

Implemented and live-tested:

- `ClickHouseConnection::establish("http://user:password@host:8123/database")`.
- Idiomatic Diesel `load`, `first`, and `execute` over ClickHouse HTTP.
- Diesel bind collection for primitive/text values, inlined as escaped ClickHouse literals while preserving literal `?` characters in SQL strings/comments.
- Row loading through `TabSeparatedWithNamesAndTypes` into Diesel `Row`/`Field` abstractions.
- Primitive numeric, bool, text, binary, and nullable row decoding.
- `Array<T>` row decoding into `Vec<T>`, `Map<K, V>` row decoding into `BTreeMap<K, V>`, and `Tuple<...>` row decoding into Rust tuples.
- String-form Decimal/Date/DateTime/UUID/IP/JSON decoding for ClickHouse text formats.
- `diesel::sql_query(...)` with `QueryableByName`.
- Explicit unsupported transaction errors.

Remaining limitations:

- `execute` returns `0` affected rows because ClickHouse HTTP does not provide Diesel-style affected counts for DDL/mutations.
- Bind inlining is textual; true binary vector parameters still need a richer transport representation.
- Complex ClickHouse values such as native decimal representations, `Dynamic`, `Variant`, and aggregate states need expanded `FromSql`/`ToSql` coverage.
- `batch_execute` uses a small SQL-aware splitter for migration-style batches; it respects semicolons in quoted literals and comments but is not intended to be a full ClickHouse SQL parser.

## Goals

- Allow idiomatic Diesel calls such as `query.load::<T>(&mut conn)?` against ClickHouse.
- Reuse the existing `ClickHouse` backend and `QueryFragment<ClickHouse>` implementations.
- Preserve Diesel's schema-driven compile-time type checks.
- Support deterministic bind handling for the SQL forms already covered by render and live tests.
- Keep ClickHouse-specific behavior explicit rather than pretending ClickHouse is a transactional OLTP database.

## Non-goals

- SQLx-style compile-time validation against a live database. Diesel does not normally do this.
- Full transaction semantics. ClickHouse has limited transaction support and common engines are not a match for Diesel's usual assumptions.
- Hiding all ClickHouse/client differences. Features like `INTO OUTFILE` and true binary vector parameters may remain client/protocol dependent.

## Protocol options

### HTTP via the official `clickhouse` crate

Pros:

- Already used in live tests.
- Mature enough for query execution, RowBinary decoding, and bind-like APIs.
- Simple deployment story; ClickHouse HTTP is widely available.

Cons:

- Diesel's `Connection` expects a backend-owned bind collector and load path; adapting another client's parameter model may be awkward.
- Some placeholders are bound as textual SQL values, which blocks true binary vector parameter tests today.
- Mapping ClickHouse errors into Diesel's `DatabaseErrorKind` will be approximate.

### Native protocol

Pros:

- Better fit for binary values and possibly more efficient result streaming.
- More direct access to ClickHouse type metadata.

Cons:

- More implementation work and a larger maintenance surface.
- Requires selecting or writing a native client layer with stable APIs.

Current recommendation: continue hardening the HTTP implementation because it validates the Diesel integration shape with the least new infrastructure. Keep the connection boundary narrow enough that a native protocol can replace the transport later.

## Bind collection

The render layer emits `?` placeholders. The current connection collects Diesel binds, uses backend type metadata to decide whether a value needs ClickHouse string-literal escaping, and inlines those values before sending SQL over HTTP. Any remaining literal `?` characters are escaped for the underlying `clickhouse` client so they are not mistaken for unbound client parameters.

Open questions:

- Whether to keep textual bind inlining long term or render ClickHouse server-side parameters (`{name: Type}`) internally.
- How to preserve type information for arrays, maps, decimals, UUIDs, IPv4/IPv6, aggregate states, vectors, `Dynamic`, and `Variant`.
- How to support true binary binds for `reinterpret(binary, 'Array(Float32)')` vector helpers.

## Loading rows

Diesel's `Queryable`/`FromSql` path needs ClickHouse `RawValue` values that line up with Diesel SQL types. The current connection decodes `TabSeparatedWithNamesAndTypes` into owned row fields, which is simple and debuggable but not the final word for every ClickHouse type. Richer `FromSql` implementations may later be backed by RowBinary or native protocol values.

Priority row types:

1. Primitive numeric, bool, text, binary.
2. Date/time and UUID.
3. Arrays, maps, tuples, nullable values.
4. Decimals and wide integers.
5. ClickHouse-specific semi-structured/geographic types.
6. Aggregate states only if there is a concrete transport representation worth exposing.

## Execution semantics

ClickHouse statements often complete asynchronously at the storage layer. The DSL already exposes `SETTINGS`, and live mutation tests use `mutations_sync = 2` for determinism. A `Connection` should not silently add settings; callers should opt in through existing query settings or connection-level defaults.

Transactions should probably be one of:

- unsupported with a clear error, or
- implemented only for ClickHouse configurations where transactional semantics are known to work.

The first implementation should prefer explicit unsupported errors over surprising partial behavior.

## Active TODOs before treating `Connection` as stable

Use this list as the post-context-clear implementation plan. Keep `docs/FEATURE_MATRIX.md` as the canonical feature-status table, but update this section while hardening `ClickHouseConnection`.

1. **Native decimal story**
   - Decide whether to add an optional `bigdecimal` or `rust_decimal` feature, or keep decimal loading string-first.
   - If a native decimal feature is added, cover `Numeric` and `Decimal32/64/128/256` with live tests.

2. **Bind strategy**
   - Current implementation collects Diesel binds and inlines escaped ClickHouse literals.
   - Investigate ClickHouse server-side parameters (`{name: Type}`) as a safer long-term HTTP path.
   - Preserve good Diesel ergonomics: ordinary `.filter(col.eq(value))` should continue to work.
   - Keep true binary vector parameters on the list; current HTTP/text path still cannot prove them.

3. **More connection live tests**
   - Nullable scalar combinations through `ClickHouseConnection` are covered.
   - Nullable arrays/maps/tuples are covered for ergonomic Diesel targets.
   - `Dynamic` and `Variant` are covered with required experimental settings.
   - Raw SQL structs via `QueryableByName` are covered for the supported semi-structured/composite cases.
   - Aggregate states only if we expose a concrete Rust representation.

4. **`batch_execute` semantics**
   - Replaced the original string split with a small SQL-aware migration splitter.
   - Keep the documented scope narrow: it respects quoted semicolons and comments, but it is not a complete ClickHouse SQL parser.

5. **Connection options/settings API**
   - Consider constructors/builders for user/password/database/options instead of only URL parsing and `with_client`.
   - Do not silently inject ClickHouse settings that alter semantics; make defaults explicit.

6. **Protocol boundary**
   - Continue hardening HTTP first.
   - Keep row loading and bind collection isolated enough that a native protocol transport can replace HTTP later if binary fidelity becomes necessary.

## Suggested implementation order

1. Decide native decimal feature direction; implement only if the added dependency/feature is worth it.
2. Investigate ClickHouse server-side parameter rendering and compare against current bind inlining.
3. Add connection builder/options ergonomics.
4. Revisit native protocol only after the HTTP-backed public API shape is proven.
