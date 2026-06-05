# Usage guide

`diesel-clickhouse` turns Diesel ASTs and ClickHouse-specific DSL nodes into ClickHouse SQL. You can either render SQL and execute it with a ClickHouse client, or use the native async `AsyncClickHouseConnection` for idiomatic Diesel `load`/`execute` calls.

> This guide explains the **model**. For copyable "how do I write this query?" recipes — raw SQL next to the equivalent Diesel, each verified by running both and asserting equal results — see the cookbook (`docs/COOKBOOK.md` in the repo, or `docs::cookbook` on docs.rs).

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

> **Bind values do not travel with `to_sql`.** `to_sql(&query)` renders only the SQL string; the Diesel bind values stay behind. When you execute that string through an external `clickhouse::Client`, *you* re-supply every value with `.bind(...)` in render order, so a reordered `filter`/`select` or a changed type silently drifts out of sync with your `.bind(...)` calls. `to_sql_with_metadata` (see below) lets a test assert the placeholder count matches. To remove the hand-binding step entirely, execute through `AsyncClickHouseConnection`, which carries the Diesel-collected bind values through for you.

## Diesel `AsyncConnection`

`AsyncClickHouseConnection` is a native async Diesel connection (a [`diesel_async::AsyncConnection`]) backed by ClickHouse's HTTP interface. Drive queries with `diesel_async`'s `RunQueryDsl` and `.await`:

```rust,ignore
use diesel::prelude::*;
use diesel_async::{AsyncConnection, RunQueryDsl};
use diesel_clickhouse::AsyncClickHouseConnection;

let mut conn = AsyncClickHouseConnection::establish(
    "http://default:password@localhost:8123/analytics",
)
.await?;

let rows: Vec<(String, i64)> = events::table
    .filter(events::tenant_id.eq("acme").and(events::success.eq(true)))
    .group_by(events::tenant_id)
    .select((events::tenant_id, diesel::dsl::count_star()))
    .load(&mut conn)
    .await?;
```

> Import `diesel_async::RunQueryDsl` explicitly. It shadows the `RunQueryDsl` that `diesel::prelude::*` brings in, so `.load`/`.first`/`.execute` resolve to the async connection's methods rather than the (inapplicable) blocking ones.

For explicit code-assembled configuration, use `ClickHouseConnectionOptions`:

```rust,ignore
use diesel_clickhouse::ClickHouseConnectionOptions;

let mut conn = ClickHouseConnectionOptions::new("http://localhost:8123")
    .user("default")
    .password("password")
    .database("analytics")
    .option("max_threads", "1")
    .connect()
    .await?;
```

The current connection supports `establish`, `AsyncClickHouseConnection::with_client(client)`, explicit `ClickHouseConnectionOptions`, `load`, `first`, `execute`, `batch_execute`, primitive/text/nullable row values, `Array<T>` into `Vec<T>`, `Map<K, V>` into `BTreeMap<K, V>`, `Tuple<...>` into Rust tuples, string-form Decimal/Date/DateTime/UUID/IP/JSON/Dynamic/Variant values, optional `BigDecimal` values with the `bigdecimal` feature, and `diesel::sql_query(...)`/`QueryableByName` for raw SQL. It sends supported Diesel-collected bind values as ClickHouse HTTP server-side parameters; ambiguous cases such as `NULL` HTTP parameters and abstract composite metadata fall back to escaped literal inlining. Concrete array binds (`Array(UInt64)`, `Array(String)`, `Array(Float32)`) are serialized by the crate as typed ClickHouse array literals and sent through the same Diesel-owned bind path. Literal `?` characters inside SQL strings/comments are preserved.

### Bootstrap and database creation

`AsyncClickHouseConnection::establish(...)` and `ClickHouseConnectionOptions::connect()` run a `SELECT 1` health check through the configured client. If you configure `.database("analytics")`, that database must already exist. For first-run bootstrap, use a database-less/admin `clickhouse::Client` for `CREATE DATABASE IF NOT EXISTS`, then create tables, then construct the database-scoped async Diesel connection for ordinary work:

```rust,ignore
use diesel_async::SimpleAsyncConnection;
use diesel_clickhouse::{
    clickhouse::{Client, sql::Identifier},
    create_table, to_sql, ClickHouseConnectionOptions, DataType, TableEngine,
};

let admin = Client::default().with_url("http://localhost:8123");
admin.query("CREATE DATABASE IF NOT EXISTS ?")
    .bind(Identifier("analytics"))
    .execute()
    .await?;

let ddl = create_table("analytics.events")
    .if_not_exists()
    .column("id", DataType::UInt64)
    .engine(TableEngine::memory());
admin.query(&to_sql(&ddl)?).execute().await?;

let mut conn = ClickHouseConnectionOptions::new("http://localhost:8123")
    .database("analytics")
    .connect()
    .await?;
conn.batch_execute("INSERT INTO events VALUES (1)").await?;
```

### Raw fragments and array binds under async execution

Raw SQL is still sometimes necessary for ClickHouse-only grammar. When executing through `AsyncClickHouseConnection`, put bind values into the raw fragment with Diesel's literal binding API instead of writing a bare `?` and binding later through an external client:

```rust,ignore
use diesel::dsl::sql;
use diesel::sql_types::{Bool, Text};

let needle = "rust";
let predicate = sql::<Bool>("positionCaseInsensitive(text, ")
    .bind::<Text, _>(needle)
    .sql(") > 0 AND text != '?' /* this ? is ignored */");

let rows = documents::table
    .filter(documents::tenant_id.eq("acme"))
    .filter(predicate)
    .load::<Document>(&mut conn)
    .await?;
```

For ClickHouse array parameters, use this crate's typed `bind` wrapper so the vector is collected by Diesel and serialized by `diesel-clickhouse`:

```rust,ignore
use diesel::dsl::sql;
use diesel::sql_types::{Bool, Text};
use diesel_clickhouse::{array_exists2, bind, lambda2};
use diesel_clickhouse::sql_types::{Array, UInt64};

type TurnIds = Array<UInt64>;
let turn_ids = vec![10_u64, 20, 30];

let by_turn = sql::<Bool>("has(")
    .bind::<TurnIds, _>(bind::<TurnIds, _>(turn_ids))
    .sql(", toUInt64(silver_turn_id))");

type TextArray = Array<Text>;
let allowed_types = vec!["question".to_owned(), "answer".to_owned()];
let allowed_roles = vec!["user".to_owned(), "assistant".to_owned()];
let allowed_pair = array_exists2(
    lambda2(
        "allowed_type",
        "allowed_role",
        "allowed_type = aspect_type AND allowed_role = speaker_role",
    ),
    bind::<TextArray, _>(allowed_types),
    bind::<TextArray, _>(allowed_roles),
);
```

Prefer `when(enabled, predicate)` for optional filters when stable SQL text is not required: disabled branches render `1` and contribute no bind values, so later binds such as `LIMIT ?` keep their intended positions.

Common ClickHouse functions that previously required raw fragments have typed helpers: `position_case_insensitive`, `length_utf8`, `left_utf8`, `null_if`, `to_float32`, `if_`, and `array_exists2` for two parallel arrays. For ordering by a computed alias, prefer `alias_ref::<ST>("score").desc()` over `sql::<ST>("score").desc()`; the helper validates and quotes the alias identifier.

### Loading structs and wide rows

For typed Diesel `select(...)` queries, derive `Queryable` and keep the Rust struct fields in select-list order:

```rust,ignore
#[derive(Queryable)]
struct TenantOverview {
    tenant_id: String,
    n: u64,
    ok: i64,
}

let rows: Vec<TenantOverview> = events::table
    .group_by(events::tenant_id)
    .select((events::tenant_id, expr_as(count(), "n"), expr_as(count_if(events::success), "ok")))
    .load(&mut conn)
    .await?;
```

For raw SQL, aliased expressions, or very wide result shapes, use `diesel::sql_query(...)` plus `QueryableByName` and annotate each field with its SQL type. This avoids Diesel's tuple arity pressure for 17+ column analytics rows and makes aliases explicit. The live test suite covers a 16-column `QueryableByName` row with `String`, `UInt64`, `UInt32`, `Nullable(Int32)`, `Float32`, `UUID`, `DateTime64`, and `Array(Float32)` fields.

```rust,ignore
#[derive(QueryableByName)]
struct DocumentHit {
    #[diesel(sql_type = diesel_clickhouse::sql_types::UInt64)]
    id: u64,
    #[diesel(sql_type = diesel_clickhouse::sql_types::Uuid)]
    uuid_value: String,
    #[diesel(sql_type = diesel_clickhouse::sql_types::DateTime64)]
    processed_at: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    source_type: Option<String>,
    #[diesel(sql_type = diesel_clickhouse::sql_types::Array<diesel::sql_types::Float>)]
    embedding: Vec<f32>,
}
```

Enable native BigDecimal decimal values with:

```toml
[dependencies]
diesel-clickhouse = { version = "...", features = ["bigdecimal"] }
```

With that feature, `bigdecimal::BigDecimal` implements `FromSql`/`ToSql` for Diesel `Numeric` and ClickHouse `Decimal32<S>`/`Decimal64<S>`/`Decimal128<S>`/`Decimal256<S>`. String-form decimal loading remains available without extra dependencies.

ClickHouse is not treated as an OLTP database: Diesel transaction APIs return a clear unsupported error. `execute` does report affected rows, though — it reads the `written_rows` field of ClickHouse's `X-ClickHouse-Summary` response trailer (running with `wait_end_of_query=1` so the count reflects the completed write), so an INSERT/mutation returns a real count. Statements ClickHouse does not count, such as DDL, report `0`.

### Async runtime and pooling

`AsyncClickHouseConnection` is a **native async** connection: it drives the async `clickhouse` client's futures directly, with no owned runtime and no `block_on`. Call `load`/`execute`/`batch_execute` from any async task with `.await` — including from inside an `async fn` on the Tokio executor (the restriction that applied to the previous blocking connection is gone).

For connection pooling it integrates with diesel-async's `bb8`, `deadpool`, and `mobc` pools. Enable the matching feature and use `AsyncDieselConnectionManager`:

```toml
[dependencies]
diesel-clickhouse = { version = "...", features = ["bb8"] }
```

```rust,ignore
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::bb8::Pool;
use diesel_clickhouse::AsyncClickHouseConnection;

let config = AsyncDieselConnectionManager::<AsyncClickHouseConnection>::new(database_url);
let pool = Pool::builder().build(config).await?;
let mut conn = pool.get().await?;
```

If you already have a configured `clickhouse::Client`, `AsyncClickHouseConnection::with_client(client)` wraps it without I/O and preserves URL, credentials, database, validation mode, and HTTP settings already applied to that client. Creating a short-lived connection per query from a cloned client is acceptable for ClickHouse HTTP while evaluating the need for a pool; use a `diesel_async` pool when you want checkout limits, shared lifecycle management, or framework integration rather than because the underlying client handle is expensive.

Connection-level options (`ClickHouseConnectionOptions::option(...)` or `client.with_setting(...)`) become default HTTP settings on every request. SQL `SETTINGS` clauses are query text and should be used when the setting is semantically part of one statement.

If you need to drive the connection from blocking/synchronous code (or run `diesel_migrations`), enable the `async-connection-wrapper` feature and wrap it in diesel-async's `AsyncConnectionWrapper`.

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

Single-row `execute` returns the number of written rows ClickHouse reports in its `X-ClickHouse-Summary` response trailer (so a single-row insert returns `1`). DDL and the background mutations behind some `ALTER ... DELETE`/`UPDATE` forms are not counted by ClickHouse and report `0`.

**Multi-row batch inserts (`.values(vec![...])`) are not expressible through Diesel's DSL.** Diesel's multi-row `BatchInsert` is hardwired to require the SQL `DEFAULT` keyword, and the only escape hatch is a backend-specific `QueryFragment` impl that the orphan rule reserves for Diesel's own backends. `to_sql`/`execute` on a Diesel batch insert will not compile. This is a hard limitation of building on Diesel's backend traits, not an oversight.

**For multi-row ingestion, use [`AsyncClickHouseConnection::insert_batch`](crate::AsyncClickHouseConnection::insert_batch)**, which drives the `clickhouse` client's native RowBinary inserter for you — one columnar request for the whole batch, the high-throughput path ClickHouse is designed for:

```rust,ignore
#[derive(clickhouse::Row, serde::Serialize)]
struct EventRow {
    id: u64,
    tenant_id: String,
}

// One columnar RowBinary request; returns the number of rows sent.
let written = conn.insert_batch("events", events).await?;

// Add per-insert operational bounds/settings when needed.
let written = conn
    .insert_batch_with_options(
        "events",
        events,
        InsertBatchOptions::new()
            .timeouts(Some(send_timeout), Some(end_timeout))
            .setting("query_id", query_id),
    )
    .await?;
```

This sends one columnar RowBinary request for the whole batch instead of N round-trips of escaped text — orders of magnitude faster than looping single-row inserts, and the recommended approach for any non-trivial write volume. Table names passed to `insert_batch`/`insert_batch_with_options` are validated as bare or database-qualified identifiers before execution.

For long-running, periodically-flushed ingestion, reach for the client's `inserter(...)` directly. [`AsyncClickHouseConnection::client`](crate::AsyncClickHouseConnection::client) exposes the configured `clickhouse::Client`, which is cheap to clone and usable from your own async code:

```rust,ignore
// `client.inserter(...)`' s `commit()`/`end()` return `Quantities` with the
// written row/byte counts, and flush on size/row/time thresholds.
let client = conn.client().clone();
```

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

Use `clickhouse_join(...)` for executable ClickHouse join SQL, especially for ClickHouse-specific grammar (`GLOBAL`, `ANY`/`ALL`/`ASOF`, `SEMI`/`ANTI`). Wrap projected columns with `source_column(...)` (or the backwards-compatible `join_column(...)` name) so the select list is type-checked instead of a hand-written string:

```rust,ignore
use diesel_clickhouse::{source_column, ClickHouseJoinDsl};

let rows: Vec<(i64, String)> = events::table
    .clickhouse_join(tenants::table)
    .any()
    .inner()
    .on(events::tenant_id.eq(tenants::tenant_id))
    .select((source_column(events::id), source_column(tenants::plan)))
    .load(&mut conn)?;
```

`source_column(events::id)` keeps the column's SQL type, so the query loads into a typed `(i64, String)` and renders the same `` `events`.`id` `` qualified SQL — replacing the old `sql::<(BigInt, Text)>("`events`.`id`, ...")` escape hatch, which discarded both the names and the types. Use `source_column_as(column, "alias")` when ClickHouse result metadata should expose an unqualified/struct-friendly field name.

Why a wrapper instead of bare columns: Diesel's `table!` macro only implements `SelectableExpression` for a column against the table itself and Diesel's *own* built-in join nodes. Rust's orphan rule prevents a third-party backend from adding that impl for arbitrary foreign columns over a custom source, so the column is wrapped in a local type that carries the same SQL type. The trade-off is that `source_column` does not verify the column's table actually appears in the source — that one check is what Diesel's built-in joins provide and what the orphan rule withholds here.

Diesel's built-in `.inner_join(...).on(...)` is type-safe but **not executable** on ClickHouse: Diesel emits a parenthesized join source (`FROM (a INNER JOIN b ON ...)`), which ClickHouse parses as a subquery (`FROM (SELECT * FROM a JOIN b ...)`) and then rejects the outer qualified column references. The parentheses come from Diesel's backend-generic `Grouped` wrapper, which cannot be overridden per backend. Use `clickhouse_join(...)` with `source_column(...)` for executable joins; treat built-in join rendering as inspection-only.

## Predicates: prefer typed expressions

Write `WHERE`/`ON`/`HAVING` constraints as typed Diesel expressions that refer to columns directly, **not** as `sql::<Bool>("my_column > 2")` strings. Typed predicates are checked against your `table!` schema: the column path must exist, the comparison must type-check, and a renamed column or wrong literal type is a compile error instead of a runtime ClickHouse error.

```rust,ignore
use diesel_clickhouse::ClickHouseJoinDsl;

// Typed — `tenant_id`/`status`/`success` must exist and the literals must match.
let rows: Vec<(i64, String)> = events::table
    .clickhouse_join(tenants::table)
    .any()
    .inner()
    .on(events::tenant_id.eq(tenants::tenant_id)) // ON requires a boolean predicate
    .select((source_column(events::id), source_column(tenants::plan)))
    .filter(events::tenant_id.eq("acme"))         // typed columns from either side
    .filter(tenants::plan.eq("enterprise"))
    .load(&mut conn)?;
```

Two things to know about predicates on the ClickHouse-specific sources:

- **`.on(...)` requires a boolean predicate** (`Bool` or `Nullable<Bool>`). `events::col.eq(other::col)` is accepted; a stray non-boolean expression is rejected at compile time rather than rendering `ON <non-bool>`.
- **`.filter(...)` is available after `.select(...)`.** Diesel does not implement `FilterDsl` for the custom source wrappers themselves (`Final`, `ClickHouseJoin`, `Prewhere`, `Sample`, `ArrayJoin`), so call `.select(...)` first — the resulting statement filters by typed columns normally. (`final_table(t).filter(...)` before a `.select(...)` does not compile.)

Note the asymmetry with projection: typed **predicates keep** Diesel's full safety, including the check that the column's table appears in the source. Typed **projections** through `source_column(...)` deliberately **trade away** that appearance check (Rust's orphan rule prevents a third-party backend from reproducing it for foreign columns over a custom source — see the Joins section). So reach for typed predicates freely; `source_column` is the narrower escape hatch.

Some constraints genuinely need raw SQL — e.g. an `ASOF` `.on(...)` that mixes equality and inequality across columns ClickHouse evaluates positionally. Use `sql::<Bool>("...")` there deliberately, not as the default.

For aggregates in the select/having position, prefer the typed helpers over `aggregate::<...>("name")` strings where one exists — for example `count()` (typed `UInt64`, ClickHouse's native row-count type) instead of `aggregate::<UInt64>("count").no_args()`, and `expr_as(expr, "alias")` to name any expression for struct-friendly result metadata.

## Custom source wrappers

ClickHouse source modifiers such as `FINAL`, `SAMPLE`, `PREWHERE`, and `ARRAY JOIN` are available as query-source wrappers. Project typed columns through them with `source_column(...)`:

```rust,ignore
use diesel_clickhouse::{final_table, prewhere, sample, source_column, source_column_as};

let source = prewhere(sample(final_table(events::table), 0.1), events::tenant_id.eq("acme"));
let rows: Vec<(i64, String)> = source
    .select((source_column(events::id), source_column_as(events::tenant_id, "tenant")))
    .filter(events::success.eq(true))
    .load(&mut conn)
    .await?;
```

Use `alias_source(source, "e")` or `.alias_source("e")` when ClickHouse-specific SQL fragments need a short source alias; identifiers are validated and rendered as backtick-quoted aliases. Apply the alias to the table-like source before `prewhere(...)` or `array_join(...)` wrappers so ClickHouse sees `FROM table FINAL AS e PREWHERE ...`, not an alias after those clauses.

## Rendered SQL metadata

`to_sql_with_metadata(&query)` returns the same SQL string as `to_sql(&query)` plus placeholder metadata outside quoted strings/comments:

```rust,ignore
let rendered = diesel_clickhouse::to_sql_with_metadata(&query)?;
assert_eq!(rendered.positional_bind_count(), 2);
assert_eq!(rendered.positional_bind_types(), &["String", "Bool"]);
assert_eq!(rendered.named_parameters(), &["allowed"]);
assert_eq!(rendered.named_parameter_details()[0].type_name, "Array(String)");
```

This helper is useful when rendering a Diesel AST with `to_sql` and then executing it through `diesel_clickhouse::clickhouse::Client`: tests can assert that every rendered positional placeholder has a matching `.bind(...)` call with the expected ClickHouse type, and every ClickHouse HTTP parameter (`{name:Type}`) has a matching `.param(...)` call. It is still a verification aid, not bind ownership; only `AsyncClickHouseConnection` carries the Diesel-collected values into execution.

## Render-only and client-dependent areas

- `INTO OUTFILE` is render-tested only. ClickHouse documents it as CLI/local-client functionality; it fails through HTTP.
- Binary vector parameter helpers render ClickHouse's `reinterpret(binary, 'Array(Float32)')` pattern, but the current HTTP live-test path binds placeholders as SQL strings rather than true binary parameters.
- `Dynamic` and `Variant` DDL may require ClickHouse experimental settings such as `allow_experimental_dynamic_type=1` and `allow_experimental_variant_type=1` on older servers.

## Compile-time checking

Diesel validates DSL expressions against Rust schema metadata from `table!`/`schema.rs`. That catches many mistakes: unknown columns, incompatible SQL types, aggregate/non-aggregate mixing, and select tuple shape mismatches.

It does not connect to a development ClickHouse database during compilation. `AsyncClickHouseConnection` can support future schema-generation tooling, but SQLx-style live query validation is not part of Diesel's normal model.
