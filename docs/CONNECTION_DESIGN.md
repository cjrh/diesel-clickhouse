# Diesel `Connection` design notes

The crate now includes an initial HTTP-backed `ClickHouseConnection`. This document records the design constraints, current scope, and remaining work so the connection can evolve without pretending ClickHouse is a regular transactional OLTP backend.

## Current spike

Implemented and live-tested:

- `ClickHouseConnection::establish("http://user:password@host:8123/database")` and explicit `ClickHouseConnectionOptions` construction.
- Idiomatic Diesel `load`, `first`, and `execute` over ClickHouse HTTP.
- Diesel bind collection for primitive/text values, sent as ClickHouse HTTP server-side parameters (`{dc_pN:Type}` plus `param_dc_pN`) where type metadata is concrete.
- Row loading through `TabSeparatedWithNamesAndTypes` into Diesel `Row`/`Field` abstractions.
- Primitive numeric, bool, text, binary, and nullable row decoding.
- `Array<T>` row decoding into `Vec<T>`, `Map<K, V>` row decoding into `BTreeMap<K, V>`, and `Tuple<...>` row decoding into Rust tuples.
- String-form Decimal/Date/DateTime/UUID/IP/JSON decoding for ClickHouse text formats, plus optional `bigdecimal` support for `Numeric` and Decimal32/64/128/256.
- `diesel::sql_query(...)` with `QueryableByName`.
- Explicit unsupported transaction errors.

Remaining limitations:

- `execute` returns `0` affected rows. ClickHouse *does* report written/affected rows in the `X-ClickHouse-Summary` HTTP response header, but the `clickhouse` client's `execute()` returns `()` and discards that header, so the count is not reachable through the current transport. Surfacing a real count would require the upstream client exposing the summary, or a native-protocol transport; bolting on a parallel HTTP request just to read the header would widen the transport boundary this design deliberately keeps narrow.
- Server-side parameters still use ClickHouse's textual HTTP parameter representation; true binary vector parameters need a richer transport representation.
- Complex ClickHouse values such as aggregate states and native binary-only representations need expanded `FromSql`/`ToSql` coverage.
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

The render layer emits `?` placeholders. The current connection collects Diesel binds, rewrites supported placeholders to ClickHouse server-side parameter syntax (`{dc_pN:Type}`), and sends values as HTTP `param_dc_pN` query options. Remaining literal `?` characters are escaped for the underlying `clickhouse` client so they are not mistaken for unbound client parameters.

The server-parameter path is the default for concrete primitive/string/date/time/UUID/IP/JSON/Dynamic and scaled decimal metadata. The connection intentionally falls back to escaped literal inlining for cases where ClickHouse HTTP parameters are not yet reliable or the backend metadata is too abstract: `NULL` values (ClickHouse 24.8 rejects `NULL` as a `Nullable(T)` HTTP parameter), Diesel `Numeric` without precision/scale, arrays/maps/tuples, `LowCardinality`, `Nested`, `Variant`, and aggregate states.

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

## Connection hardening decisions and status

This section records the post-context-clear hardening decisions. Keep `docs/FEATURE_MATRIX.md` as the canonical feature-status table; use this section for connection-specific design rationale and future revisit points.

All active connection-hardening TODOs are complete for the initial HTTP-backed `ClickHouseConnection`. Remaining notes below are future revisit triggers, not blockers for treating the current connection surface as stable.

1. ✅ **Native decimal story — complete**
   - Decision: add an optional `bigdecimal` feature while keeping string-form decimals as the dependency-free baseline.
   - Implemented `BigDecimal` `FromSql`/`ToSql` support for Diesel `Numeric` and ClickHouse `Decimal32/64/128/256`.
   - Covered `Numeric`, `Decimal32/64/128/256`, and a `BigDecimal` raw-SQL bind with live tests under `--features bigdecimal`.
   - Note: Diesel's coherence rules make direct `AsExpression` impls for external `BigDecimal` and custom decimal SQL types impractical downstream; raw SQL binds work, and Diesel's own `Numeric` ergonomics come from `diesel/numeric`.

2. ✅ **Bind strategy — complete**
   - Decision: use ClickHouse server-side HTTP parameters by default when metadata provides a concrete type.
   - Implemented generated placeholders (`{dc_pN:Type}`) and per-query HTTP parameters (`param_dc_pN`) for supported binds.
   - Preserved good Diesel ergonomics: ordinary `.filter(col.eq(value))` continues to work, and unsupported/ambiguous cases fall back to the previous escaped literal inlining path.
   - Covered server-side text binds, literal question marks, and NULL fallback with live tests.
   - Future revisit: true binary vector parameters remain out of scope for the HTTP/text parameter path.

3. ✅ **More connection live tests — complete**
   - Nullable scalar combinations through `ClickHouseConnection` are covered.
   - Nullable arrays/maps/tuples are covered for ergonomic Diesel targets.
   - `Dynamic` and `Variant` are covered with required experimental settings.
   - Raw SQL structs via `QueryableByName` are covered for the supported semi-structured/composite cases.
   - Aggregate states remain future work only if a concrete Rust representation is exposed.

4. ✅ **`batch_execute` semantics — complete**
   - Replaced the original string split with a small SQL-aware migration splitter.
   - Keep the documented scope narrow: it respects quoted semicolons and comments, but it is not a complete ClickHouse SQL parser.

5. ✅ **Connection options/settings API — complete**
   - Added `ClickHouseConnectionOptions` as the explicit configuration abstraction for URL, user, password, database, and HTTP query options/settings.
   - `ClickHouseConnection::establish` delegates to `ClickHouseConnectionOptions::from_url`, preserving existing URL parsing behavior while exposing builder-style setters for code-assembled configuration.
   - No ClickHouse settings that alter semantics are silently injected; callers add options/settings explicitly.

6. ✅ **Protocol boundary — complete for HTTP-first design**
   - Decision: do not introduce a native-protocol abstraction yet.
   - The HTTP-backed public API shape is now proven enough for the initial connection: URL/options construction, loading, execution, bind handling, unsupported transactions, and migration-style batches are covered by focused tests.
   - Keep native protocol as future work only if binary fidelity becomes necessary for true binary vector parameters, aggregate states, or other values that cannot be represented well through ClickHouse HTTP text formats.
   - Until there is a concrete native client target, avoid a premature transport trait; keep row loading, bind preparation, and HTTP execution as internal helpers that can be extracted later without changing the public `ClickHouseConnection` API.

## Suggested implementation order

No immediate connection-hardening tasks remain in this document. Future work should be demand-driven: revisit native protocol/binary transport only when a supported feature requires fidelity the HTTP path cannot provide.
