# Diesel `AsyncConnection` design notes

The crate includes a native async, HTTP-backed `AsyncClickHouseConnection` that implements [`diesel_async::AsyncConnection`] directly on top of the async `clickhouse` client's futures (no owned runtime, no `block_on`). This document records the design constraints, current scope, and remaining work so the connection can evolve without pretending ClickHouse is a regular transactional OLTP backend.

## Current spike

Implemented and live-tested:

- `AsyncClickHouseConnection::establish("http://user:password@host:8123/database")` and explicit `ClickHouseConnectionOptions` construction.
- Idiomatic Diesel `load`, `first`, and `execute` over ClickHouse HTTP.
- Diesel bind collection for primitive/text values and concrete arrays (`Array(UInt64)`, `Array(String)`, `Array(Float32)`), sent as ClickHouse HTTP server-side parameters (`{dc_pN:Type}` plus `param_dc_pN`) where type metadata is concrete.
- Row loading through `TabSeparatedWithNamesAndTypes` into Diesel `Row`/`Field` abstractions.
- Primitive numeric, bool, text, binary, and nullable row decoding.
- `Array<T>` row decoding into `Vec<T>`, `Map<K, V>` row decoding into `BTreeMap<K, V>`, and `Tuple<...>` row decoding into Rust tuples.
- String-form Decimal/Date/DateTime/UUID/IP/JSON decoding for ClickHouse text formats, plus optional `bigdecimal` support for `Numeric` and Decimal32/64/128/256.
- `diesel::sql_query(...)` with `QueryableByName`.
- `AsyncClickHouseConnection::insert_batch(table, rows)` for high-throughput multi-row ingestion through the `clickhouse` client's native RowBinary inserter (see "Writing data" in `docs/USAGE.md`).
- Explicit unsupported transaction errors.

Remaining limitations:

- `execute` reports written rows only when ClickHouse provides them. Statement execution reads the `written_rows` field of the `X-ClickHouse-Summary` response trailer that the `clickhouse` client exposes on its byte cursor, so INSERT/mutation `execute` calls return a real count. The execute path runs with `wait_end_of_query=1` so the summary reflects the completed write rather than mid-flight progress. Statements ClickHouse does not count — DDL, and the background mutations behind some `ALTER ... DELETE`/`UPDATE` forms — still report `0`.
- Server-side parameters still use ClickHouse's textual HTTP parameter representation; true binary vector parameters need a richer transport representation.
- Complex ClickHouse values such as aggregate states and native binary-only representations need expanded `FromSql`/`ToSql` coverage.
- `batch_execute` uses a small SQL-aware splitter for migration-style batches; it respects semicolons in quoted literals and comments but is not intended to be a full ClickHouse SQL parser.

## Goals

- Allow idiomatic Diesel calls such as `query.load::<T>(&mut conn).await?` against ClickHouse.
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

The render layer emits `?` placeholders. The current connection collects Diesel binds, rewrites supported placeholders to ClickHouse server-side parameter syntax (`{dc_pN:Type}`), and sends values as HTTP `param_dc_pN` query options. Remaining literal `?` characters are escaped for the underlying `clickhouse` client so they are not mistaken for unbound client parameters.

The server-parameter path is the default for concrete primitive/string/date/time/UUID/IP/JSON/Dynamic, scaled decimal metadata, and arrays whose element metadata can be rendered as `Array(T)`. The connection intentionally falls back to escaped literal inlining for cases where ClickHouse HTTP parameters are not yet reliable or the backend metadata is too abstract: `NULL` values (ClickHouse 24.8 rejects `NULL` as a `Nullable(T)` HTTP parameter), Diesel `Numeric` without precision/scale, maps/tuples, `LowCardinality`, `Nested`, `Variant`, and aggregate states.

Open questions:

- Whether to keep textual bind inlining long term or render ClickHouse server-side parameters (`{name: Type}`) internally.
- How to preserve type information for maps, decimals, UUIDs, IPv4/IPv6, aggregate states, `Dynamic`, and `Variant` beyond the currently supported HTTP text representation.
- How to support true binary binds for `reinterpret(binary, 'Array(Float32)')` vector helpers.

## Loading rows

Diesel's `Queryable`/`FromSql` path needs ClickHouse `RawValue` values that line up with Diesel SQL types. The connection decodes `TabSeparatedWithNamesAndTypes` into owned row fields, which is simple, universal, and debuggable: every ClickHouse type — including the awkward ones — has a human-readable text form, so a single text parser plus the text-oriented `FromSql` impls cover the whole type surface.

### Why not RowBinary (yet)

A `RowBinaryWithNamesAndTypes` transport would avoid text parsing and is the obvious perf lever. It was evaluated and **deliberately deferred**. The findings, so a future implementer does not re-derive them:

- The header is `varint(n_columns)`, then `n` length-prefixed column names, then `n` length-prefixed **type strings** — so each column's type is known before decoding rows. Header parsing is easy.
- Most types are straightforward binary, and two feared cases are trivial: `RowBinary` writes `LowCardinality(T)` as plain `T` (no dictionary), and `Nullable(T)` as a single `0/1` flag byte followed by the value only when non-null.
- The blocker is the **modern/semi-structured types**: `Dynamic` prefixes each value with an embedded binary-encoded type spec; `Variant` uses a discriminator; `JSON` and `AggregateFunction` states are worse and version-specific. The load path serves these today (via `sql_query`/`QueryableByName`), and a response is a single format chosen at request time — you cannot mix binary for scalars and text for the hard types within one query. So a binary transport must either hand-implement every exotic decoder (real silent-corruption risk, pinned to a server version) or regress types that currently work.
- Conclusion: keep text as the universal default. If RowBinary is revisited, the sound shape is **binary for the supported types with a transparent text fallback** when the result header contains an unsupported type — a feature/opt-in, not a replacement of the text path.

Priority row types (for any future richer transport):

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

## Connection hardening decisions and status

This section records the post-context-clear hardening decisions. Keep `docs/FEATURE_MATRIX.md` as the canonical feature-status table; use this section for connection-specific design rationale and future revisit points.

All active connection-hardening TODOs are complete for the initial HTTP-backed `AsyncClickHouseConnection`. Remaining notes below are future revisit triggers, not blockers for treating the current connection surface as stable.

1. ✅ **Native decimal story — complete**
   - Decision: add an optional `bigdecimal` feature while keeping string-form decimals as the dependency-free baseline.
   - Implemented `BigDecimal` `FromSql`/`ToSql` support for Diesel `Numeric` and ClickHouse `Decimal32/64/128/256`.
   - Covered `Numeric`, `Decimal32/64/128/256`, and a `BigDecimal` raw-SQL bind with live tests under `--features bigdecimal`.
   - Note: Diesel's coherence rules make direct `AsExpression` impls for external `BigDecimal` and custom decimal SQL types impractical downstream; raw SQL binds work, and Diesel's own `Numeric` ergonomics come from `diesel/numeric`.

2. ✅ **Bind strategy — complete**
   - Decision: use ClickHouse server-side HTTP parameters by default when metadata provides a concrete type.
   - Implemented generated placeholders (`{dc_pN:Type}`) and per-query HTTP parameters (`param_dc_pN`) for supported binds.
   - Preserved good Diesel ergonomics: ordinary `.filter(col.eq(value))` continues to work, and unsupported/ambiguous cases fall back to the previous escaped literal inlining path.
   - Covered server-side text binds, typed array binds, literal question marks, optional `when(...)` branches, and NULL fallback with live tests.
   - Future revisit: true binary vector parameters remain out of scope for the HTTP/text parameter path.

3. ✅ **More connection live tests — complete**
   - Nullable scalar combinations through `AsyncClickHouseConnection` are covered.
   - Nullable arrays/maps/tuples are covered for ergonomic Diesel targets.
   - `Dynamic` and `Variant` are covered with required experimental settings.
   - Raw SQL structs via `QueryableByName` are covered for the supported semi-structured/composite cases.
   - Aggregate states remain future work only if a concrete Rust representation is exposed.

4. ✅ **`batch_execute` semantics — complete**
   - Replaced the original string split with a small SQL-aware migration splitter.
   - Keep the documented scope narrow: it respects quoted semicolons and comments, but it is not a complete ClickHouse SQL parser.

5. ✅ **Connection options/settings API — complete**
   - Added `ClickHouseConnectionOptions` as the explicit configuration abstraction for URL, user, password, database, and HTTP query options/settings.
   - `AsyncClickHouseConnection::establish` delegates to `ClickHouseConnectionOptions::from_url`, preserving existing URL parsing behavior while exposing builder-style setters for code-assembled configuration.
   - No ClickHouse settings that alter semantics are silently injected; callers add options/settings explicitly.

6. ✅ **Protocol boundary — complete for HTTP-first design**
   - Decision: do not introduce a native-protocol abstraction yet.
   - The HTTP-backed public API shape is now proven enough for the initial connection: URL/options construction, loading, execution, bind handling, unsupported transactions, and migration-style batches are covered by focused tests.
   - Keep native protocol as future work only if binary fidelity becomes necessary for true binary vector parameters, aggregate states, or other values that cannot be represented well through ClickHouse HTTP text formats.
   - Until there is a concrete native client target, avoid a premature transport trait; keep row loading, bind preparation, and HTTP execution as internal helpers that can be extracted later without changing the public `AsyncClickHouseConnection` API.

## Suggested implementation order

No immediate connection-hardening tasks remain in this document. Future work should be demand-driven: revisit native protocol/binary transport only when a supported feature requires fidelity the HTTP path cannot provide.
