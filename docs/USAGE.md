# Usage guide

`diesel-clickhouse` turns Diesel ASTs and ClickHouse-specific DSL nodes into ClickHouse SQL. You can either render SQL and execute it with a ClickHouse client, or use the initial synchronous `ClickHouseConnection` for idiomatic Diesel `load`/`execute` calls.

## Render and execute

```rust,ignore
use clickhouse::Client;
use diesel::prelude::*;
use diesel_clickhouse::{count_if, quantile, to_sql, ClickHouseQueryDsl};

let query = events::table
    .filter(events::tenant_id.eq("acme"))
    .group_by(events::tenant_id)
    .select((
        events::tenant_id,
        count_if(events::success),
        quantile(0.95, events::latency_ms),
    ))
    .limit_by_col(10, "tenant_id");

let sql = to_sql(&query)?;
let rows: Vec<(String, u64, f64)> = Client::default()
    .with_url("http://localhost:8123")
    .query(&sql)
    .bind("acme")
    .fetch_all()
    .await?;
```

Rendered SQL uses backtick identifiers and `?` bind placeholders. Bind values are supplied to the external ClickHouse client in the same order Diesel rendered them.

## Diesel `Connection`

`ClickHouseConnection` is a synchronous Diesel connection backed by ClickHouse's HTTP interface:

```rust,ignore
use diesel::prelude::*;
use diesel_clickhouse::ClickHouseConnection;

let mut conn = ClickHouseConnection::establish(
    "http://default:password@localhost:8123/analytics",
)?;

let rows: Vec<(String, i64)> = events::table
    .filter(events::tenant_id.eq("acme").and(events::success.eq(true)))
    .group_by(events::tenant_id)
    .select((events::tenant_id, diesel::dsl::count_star()))
    .load(&mut conn)?;
```

For explicit code-assembled configuration, use `ClickHouseConnectionOptions`:

```rust,ignore
use diesel_clickhouse::ClickHouseConnectionOptions;

let mut conn = ClickHouseConnectionOptions::new("http://localhost:8123")
    .user("default")
    .password("password")
    .database("analytics")
    .option("max_threads", "1")
    .connect()?;
```

The current connection supports `establish`, explicit `ClickHouseConnectionOptions`, `load`, `first`, `execute`, `batch_execute`, primitive/text/nullable row values, `Array<T>` into `Vec<T>`, `Map<K, V>` into `BTreeMap<K, V>`, `Tuple<...>` into Rust tuples, string-form Decimal/Date/DateTime/UUID/IP/JSON/Dynamic/Variant values, optional `BigDecimal` values with the `bigdecimal` feature, and `diesel::sql_query(...)`/`QueryableByName` for raw SQL. It sends supported Diesel-collected bind values as ClickHouse HTTP server-side parameters; ambiguous cases such as `NULL` HTTP parameters and abstract composite metadata fall back to escaped literal inlining. Literal `?` characters inside SQL strings/comments are preserved.

Enable native BigDecimal decimal values with:

```toml
[dependencies]
diesel-clickhouse = { version = "...", features = ["bigdecimal"] }
```

With that feature, `bigdecimal::BigDecimal` implements `FromSql`/`ToSql` for Diesel `Numeric` and ClickHouse `Decimal32<S>`/`Decimal64<S>`/`Decimal128<S>`/`Decimal256<S>`. String-form decimal loading remains available without extra dependencies.

ClickHouse is not treated as an OLTP database: Diesel transaction APIs return a clear unsupported error, and `execute` returns `0`. ClickHouse *does* report written/affected rows in the `X-ClickHouse-Summary` response header, but the `clickhouse` client's `execute()` returns `()` and discards that header, so the count is not reachable through the current transport.

### Blocking I/O and async

`ClickHouseConnection` is a **blocking** connection (like Diesel's `PgConnection`). It owns a current-thread Tokio runtime and drives the async `clickhouse` client with `block_on`. Tokio forbids `block_on` — and dropping a runtime — from inside an active async task, so do not call `load`/`execute`/`batch_execute`, or drop the connection, directly from an `async fn` running on the Tokio executor; that panics with "Cannot start a runtime from within a runtime". Run queries from a blocking context instead — synchronous code, a dedicated thread, an `r2d2` pool, or `spawn_blocking`:

```rust,ignore
let rows = tokio::task::spawn_blocking(move || {
    events::table.select(events::id).load::<i64>(&mut conn)
})
.await??;
```

A native `diesel-async` `AsyncConnection` is the idiomatic path for async callers and is candidate future work.

## Writing data (inserts)

ClickHouse is an analytics store: it is built for large, infrequent, columnar batch inserts, not row-at-a-time OLTP writes. That shapes what `diesel-clickhouse` supports.

**Single-row inserts work through Diesel** and execute over the connection:

```rust,ignore
// Explicit-column tuple form.
diesel::insert_into(tenants::table)
    .values((tenants::tenant_id.eq("acme"), tenants::plan.eq("pro")))
    .execute(&mut conn)?;

// `#[derive(Insertable)]` struct form. The attribute is required on ClickHouse.
#[derive(Insertable)]
#[diesel(table_name = tenants, treat_none_as_default_value = false)]
struct NewTenant<'a> {
    tenant_id: &'a str,
    plan: &'a str,
}

diesel::insert_into(tenants::table)
    .values(&NewTenant { tenant_id: "beta", plan: "free" })
    .execute(&mut conn)?;
```

`#[diesel(treat_none_as_default_value = false)]` is **required** on ClickHouse. ClickHouse has no SQL `DEFAULT` keyword in `INSERT ... VALUES`, and the defaultable insert values Diesel emits by default only render on backends that support the keyword (or via a backend-specific impl, which Rust's orphan rule forbids a third-party backend from writing). Without the attribute, a `#[derive(Insertable)]` struct will not compile against this backend.

**Multi-row batch inserts (`.values(vec![...])`) are not supported through Diesel.** Diesel's multi-row `BatchInsert` is hardwired to require the SQL `DEFAULT` keyword, and the only escape hatch is a backend-specific `QueryFragment` impl that the orphan rule reserves for Diesel's own backends. `to_sql`/`execute` on a batch insert will not compile. This is a hard limitation of building on Diesel's backend traits, not an oversight.

**For real ingestion, use the `clickhouse` client's RowBinary inserter**, which is also the high-throughput path ClickHouse is designed for. The connection exposes its configured client via [`ClickHouseConnection::client`]:

```rust,ignore
use clickhouse::Row;
use serde::Serialize;

#[derive(Row, Serialize)]
struct EventRow {
    id: u64,
    tenant_id: String,
}

// `conn.client()` returns the configured `clickhouse::Client`; it is cheap to
// clone and can be used from your own async code.
let client = conn.client().clone();
let mut insert = client.insert::<EventRow>("events")?;
for row in batch {
    insert.write(&row).await?;
}
insert.end().await?; // one batched RowBinary request

// For long-running, periodically-flushed ingestion use `client.inserter(...)`.
```

This sends one columnar RowBinary request for the whole batch instead of N round-trips of escaped text — orders of magnitude faster than looping single-row inserts, and the recommended approach for any non-trivial write volume.

## Schema declarations

Use Diesel's `table!` macro with this crate's SQL type markers for ClickHouse-only types:

```rust,ignore
diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    events (id) {
        id -> UInt64,
        tenant_id -> Text,
        created_at -> Timestamp,
        tags -> Array<Text>,
        attrs -> Map<Text, Text>,
        payload -> Json,
    }
}
```

Diesel checks the Rust query shape against this schema at compile time. This is Diesel's normal schema-driven checking, not SQLx-style live database validation during compilation.

## DDL

ClickHouse DDL is available through explicit builders:

```rust,ignore
use diesel_clickhouse::{create_table, merge_tree, Column, DataType};

let ddl = create_table("analytics.events")
    .if_not_exists()
    .column("id", DataType::UInt64)
    .column("tenant_id", DataType::low_cardinality(DataType::String))
    .column_def(Column::new("payload", DataType::String).codec("ZSTD(1)"))
    .engine(
        merge_tree()
            .partition_by(["toYYYYMM(created_at)"])
            .order_by(["tenant_id", "id"]),
    );

let sql = diesel_clickhouse::to_sql(&ddl)?;
```

`ALTER TABLE` helpers cover common column/index/projection changes, mutations, and partition operations. Complex clauses still accept raw SQL fragments where ClickHouse's grammar is broader than a useful typed Rust enum.

## Joins

Use `clickhouse_join(...)` for executable ClickHouse join SQL, especially for ClickHouse-specific grammar (`GLOBAL`, `ANY`/`ALL`/`ASOF`, `SEMI`/`ANTI`). Wrap projected columns with `join_column(...)` so the select list is type-checked instead of a hand-written string:

```rust,ignore
use diesel_clickhouse::{join_column, ClickHouseJoinDsl};

let rows: Vec<(i64, String)> = events::table
    .clickhouse_join(tenants::table)
    .any()
    .inner()
    .on(events::tenant_id.eq(tenants::tenant_id))
    .select((join_column(events::id), join_column(tenants::plan)))
    .load(&mut conn)?;
```

`join_column(events::id)` keeps the column's SQL type, so the query loads into a typed `(i64, String)` and renders the same `` `events`.`id` `` qualified SQL — replacing the old `sql::<(BigInt, Text)>("`events`.`id`, ...")` escape hatch, which discarded both the names and the types. The `ON`/`USING` constraint and any `filter`/`order` predicates already use real, type-checked columns.

Why a wrapper instead of bare columns: Diesel's `table!` macro only implements `SelectableExpression` for a column against Diesel's *own* built-in join nodes. Rust's orphan rule prevents a third-party backend from adding that impl for arbitrary foreign columns over a custom join source, so the column is wrapped in a local type that carries the same SQL type. The trade-off is that `join_column` does not verify the column's table actually appears in the join — that one check is what Diesel's built-in joins provide and what the orphan rule withholds here.

Diesel's built-in `.inner_join(...).on(...)` is type-safe but **not executable** on ClickHouse: Diesel emits a parenthesized join source (`FROM (a INNER JOIN b ON ...)`), which ClickHouse parses as a subquery (`FROM (SELECT * FROM a JOIN b ...)`) and then rejects the outer qualified column references. The parentheses come from Diesel's backend-generic `Grouped` wrapper, which cannot be overridden per backend. Use `clickhouse_join(...)` with `join_column(...)` for executable joins; treat built-in join rendering as inspection-only.

## Render-only and client-dependent areas

- `INTO OUTFILE` is render-tested only. ClickHouse documents it as CLI/local-client functionality; it fails through HTTP.
- Binary vector parameter helpers render ClickHouse's `reinterpret(binary, 'Array(Float32)')` pattern, but the current HTTP live-test path binds placeholders as SQL strings rather than true binary parameters.
- `Dynamic` and `Variant` DDL may require ClickHouse experimental settings such as `allow_experimental_dynamic_type=1` and `allow_experimental_variant_type=1` on older servers.

## Compile-time checking

Diesel validates DSL expressions against Rust schema metadata from `table!`/`schema.rs`. That catches many mistakes: unknown columns, incompatible SQL types, aggregate/non-aggregate mixing, and select tuple shape mismatches.

It does not connect to a development ClickHouse database during compilation. `ClickHouseConnection` can support future schema-generation tooling, but SQLx-style live query validation is not part of Diesel's normal model.
