# Future Diesel `Connection` design notes

The current crate intentionally stops at SQL rendering. This document records the design constraints for a later Diesel `Connection` implementation so the render layer can remain stable and the connection work can start from explicit trade-offs.

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

Initial recommendation: prototype HTTP first because it validates the Diesel integration shape with the least new infrastructure. Keep the connection boundary narrow enough that a native protocol can replace the transport later.

## Bind collection

The render layer currently emits `?` placeholders. A `Connection` implementation needs a bind collector that stores values in order and passes them to the transport in a representation ClickHouse accepts.

Open questions:

- Whether to continue using `?` placeholders or render ClickHouse server-side parameters (`{name: Type}`) internally.
- How to preserve type information for arrays, maps, decimals, UUIDs, IPv4/IPv6, aggregate states, vectors, `Dynamic`, and `Variant`.
- How to support true binary binds for `reinterpret(binary, 'Array(Float32)')` vector helpers.

## Loading rows

Diesel's `Queryable`/`FromSql` path will need ClickHouse `RawValue` values that line up with Diesel SQL types. The current serialization/deserialization support is intentionally textual and minimal; a connection implementation will likely need richer `FromSql` implementations backed by RowBinary or native protocol values.

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

## Suggested implementation order

1. Prototype `ClickHouseConnection` over HTTP for simple `SELECT` loading of primitives.
2. Add bind collection for primitive placeholders.
3. Add `execute` for DDL and mutations.
4. Expand `FromSql`/`ToSql` coverage based on existing live-test types.
5. Add transaction/metadata APIs as explicit unsupported stubs where ClickHouse cannot match Diesel expectations.
6. Revisit native protocol only after the public `Connection` API shape is proven.
