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

The current connection supports `establish`, `load`, `first`, `execute`, `batch_execute`, primitive/text/nullable row values, `Array<T>` into `Vec<T>`, `Map<K, V>` into `BTreeMap<K, V>`, `Tuple<...>` into Rust tuples, string-form Decimal/Date/DateTime/UUID/IP/JSON/Dynamic/Variant values, and `diesel::sql_query(...)`/`QueryableByName` for raw SQL. It inlines Diesel-collected bind values into escaped ClickHouse SQL literals before sending the query over HTTP; literal `?` characters inside SQL strings/comments are preserved.

ClickHouse is not treated as an OLTP database: Diesel transaction APIs return a clear unsupported error, and `execute` returns `0` because ClickHouse HTTP does not provide a conventional affected-row count for DDL and mutations.

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

Use `clickhouse_join(...)` for executable ClickHouse join SQL, especially for ClickHouse-specific grammar:

```rust,ignore
use diesel_clickhouse::ClickHouseJoinDsl;

let query = events::table
    .clickhouse_join(tenants::table)
    .global()
    .any()
    .inner()
    .using(["tenant_id"])
    .select(diesel::dsl::sql::<(diesel::sql_types::BigInt, diesel::sql_types::Text)>(
        "`events`.`id`, `tenants`.`plan`",
    ));
```

Current limitation: Diesel table columns are not automatically selectable from `ClickHouseJoin` sources, so select lists usually use `sql::<...>(...)`.

Diesel's built-in `.inner_join(...).on(...)` does render, but Diesel emits a parenthesized join source (`FROM (a INNER JOIN b ON ...)`) that ClickHouse rejects in executable queries. Treat built-in join rendering as documentation/inspection only for now.

## Render-only and client-dependent areas

- `INTO OUTFILE` is render-tested only. ClickHouse documents it as CLI/local-client functionality; it fails through HTTP.
- Binary vector parameter helpers render ClickHouse's `reinterpret(binary, 'Array(Float32)')` pattern, but the current HTTP live-test path binds placeholders as SQL strings rather than true binary parameters.
- `Dynamic` and `Variant` DDL may require ClickHouse experimental settings such as `allow_experimental_dynamic_type=1` and `allow_experimental_variant_type=1` on older servers.

## Compile-time checking

Diesel validates DSL expressions against Rust schema metadata from `table!`/`schema.rs`. That catches many mistakes: unknown columns, incompatible SQL types, aggregate/non-aggregate mixing, and select tuple shape mismatches.

It does not connect to a development ClickHouse database during compilation. `ClickHouseConnection` can support future schema-generation tooling, but SQLx-style live query validation is not part of Diesel's normal model.
