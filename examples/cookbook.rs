//! Executable cookbook for `diesel-clickhouse`.
//!
//! Each "recipe" shows a piece of ClickHouse SQL next to the Diesel query that
//! produces it. The example then runs **both**: the raw SQL through the official
//! `clickhouse` client and the Diesel query through `AsyncClickHouseConnection`,
//! and asserts the two return identical rows. Generating `docs/COOKBOOK.md` is
//! therefore a live verification that every recipe's ORM form really is
//! equivalent to its hand-written SQL — a wrong recipe fails the build.
//!
//! ```text
//! CLICKHOUSE_URL=http://default:password@localhost:8123/default \
//!   cargo run --example cookbook -- --write docs/COOKBOOK.md
//! ```

use std::env;
use std::error::Error;
use std::fmt::Debug;
use std::fs;

use diesel::prelude::*;
// Explicit import shadows the `RunQueryDsl` from diesel's prelude glob so the
// async connection's `.load`/`.first` resolve unambiguously.
use diesel_async::{RunQueryDsl, SimpleAsyncConnection};
use diesel_clickhouse::{
    ClickHouseConnectionOptions, ClickHouseJoinDsl, DataType, TableEngine, alias_ref,
    array_exists2, bind, clickhouse, count, count_if, create_table, expr_as, final_table, has,
    lambda2, source_column, to_sql, to_sql_with_metadata, vector_dot_product_f32, when,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "cookbook_events"]
    events (id) {
        id -> UInt64,
        tenant_id -> Text,
        success -> Bool,
        latency_ms -> Double,
        tags -> Array<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "cookbook_tenants"]
    tenants (tenant_id) {
        tenant_id -> Text,
        plan -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "cookbook_latest"]
    latest (id) {
        id -> UInt64,
        status -> Text,
        version -> UInt64,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "cookbook_documents"]
    documents (id) {
        id -> UInt64,
        tenant_id -> Text,
        text -> Text,
        source_type -> Text,
        processed_at -> DateTime64,
        embedding -> Array<Float>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(events, tenants);

// A row for the batch-insert recipe.
#[derive(clickhouse::Row, serde::Serialize)]
struct IngestRow {
    id: u64,
    tenant_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--write" {
            output = args.next();
        }
    }

    let database_url = env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://default:password@localhost:8123/default".to_owned());
    let mut conn = ClickHouseConnectionOptions::from_url(&database_url)?
        .connect()
        .await?;
    // A clone of the configured client runs the raw-SQL side of each parity
    // check. The clone is cheap and leaves `conn` free for the Diesel side.
    let client = conn.client().clone();

    conn.batch_execute(SETUP_SQL).await?;

    let mut doc = CookbookDoc::new();
    intro(&mut doc);

    // ----- Recipe: database bootstrap ---------------------------------------
    doc.recipe(
        "Bootstrap a database, then use `AsyncClickHouseConnection`",
        "Create the database with the underlying `clickhouse` client before you \
         build a database-scoped async Diesel connection. This keeps admin DDL \
         explicit, then moves ordinary reads/writes back to \
         `AsyncClickHouseConnection` once the database exists.",
    );
    doc.diesel(R_BOOTSTRAP_RUST);
    let bootstrap_database = "cookbook_bootstrap";
    let bootstrap_table = "events";
    client
        .query("DROP DATABASE IF EXISTS ?")
        .bind(clickhouse::sql::Identifier(bootstrap_database))
        .execute()
        .await?;
    client
        .query("CREATE DATABASE IF NOT EXISTS ?")
        .bind(clickhouse::sql::Identifier(bootstrap_database))
        .execute()
        .await?;
    let bootstrap_ddl = create_table(format!("{bootstrap_database}.{bootstrap_table}"))
        .if_not_exists()
        .column("id", DataType::UInt64)
        .engine(TableEngine::memory());
    let bootstrap_ddl_sql = to_sql(&bootstrap_ddl)?;
    client.query(&bootstrap_ddl_sql).execute().await?;

    let mut bootstrap_url = url::Url::parse(&database_url)?;
    bootstrap_url.set_path(bootstrap_database);
    let mut bootstrap_conn = ClickHouseConnectionOptions::from_url(bootstrap_url.as_str())?
        .connect()
        .await?;
    bootstrap_conn
        .batch_execute("INSERT INTO events VALUES (1)")
        .await?;
    #[derive(QueryableByName)]
    struct BootstrapCount {
        #[diesel(sql_type = diesel_clickhouse::sql_types::UInt64)]
        count: u64,
    }
    let count_row: BootstrapCount = diesel::sql_query("SELECT count() AS count FROM events")
        .get_result(&mut bootstrap_conn)
        .await?;
    assert_eq!(count_row.count, 1);
    doc.rendered(&bootstrap_ddl_sql);
    doc.text_output(&format!(
        "bootstrapped database {bootstrap_database:?}; async connection read count: {}",
        count_row.count
    ));
    drop(bootstrap_conn);
    client
        .query("DROP DATABASE IF EXISTS ?")
        .bind(clickhouse::sql::Identifier(bootstrap_database))
        .execute()
        .await?;

    // ----- Recipe: typed filters --------------------------------------------
    doc.recipe(
        "Type-safe filters instead of `sql::<Bool>`",
        "Write `WHERE` constraints as typed column expressions. The column path \
         and the literal type are checked against your `table!` schema, so a \
         renamed column or wrong literal type is a compile error — unlike a \
         `sql::<Bool>(\"tenant_id = 'acme'\")` string, which is opaque to the \
         compiler.",
    );
    doc.sql(R_FILTER_SQL);
    doc.diesel(R_FILTER_RUST);
    let orm: Vec<(u64, String)> = events::table
        .filter(events::tenant_id.eq("acme"))
        .filter(events::success.eq(true))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_FILTER_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .filter(events::tenant_id.eq("acme"))
            .filter(events::success.eq(true))
            .select((events::id, events::tenant_id))
            .order(events::id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: migrate external binds to async connection ---------------
    doc.recipe(
        "Migrating from `to_sql` + external binds to `AsyncClickHouseConnection`",
        "Rendering with `to_sql` gives you a SQL string, but the bind values stay \
         in Diesel's AST. If you execute that string through another client, a \
         human must re-supply every value in render order. Executing the same \
         AST through `AsyncClickHouseConnection` removes that manual ordering \
         step: Diesel collects the tenant and limit binds and the connection \
         sends them with the query.",
    );
    doc.diesel(R_MIGRATE_RUST);
    let migration_query = events::table
        .filter(events::tenant_id.eq("acme"))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .limit(2_i64);
    let rendered = to_sql_with_metadata(&migration_query)?;
    assert_eq!(rendered.positional_bind_count(), 2);
    let raw: Vec<(u64, String)> = client
        .query(&rendered.sql)
        .bind("acme")
        .bind(2_i64)
        .fetch_all()
        .await?;
    let orm: Vec<(u64, String)> = events::table
        .filter(events::tenant_id.eq("acme"))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .limit(2_i64)
        .load(&mut conn)
        .await?;
    doc.rendered(&rendered.sql);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: bind() against an unsigned column ------------------------
    doc.recipe(
        "Bind a Rust value to a ClickHouse-only column type",
        "`cookbook_events.id` is a `UInt64`. Diesel cannot bind a bare `u64` \
         against ClickHouse's unsigned types — its blanket `AsExpression` impl \
         makes a hand-written one a coherence conflict in every crate — so \
         `id.gt(after)` does not compile and you would otherwise reach for the \
         untyped `sql::<UInt64>(\"?\")`. `bind(value)` wraps the value as a typed \
         parameter instead: the target SQL type is inferred from the column, the \
         value is type-checked against it, and it renders as a real `?` bind.",
    );
    doc.sql(R_BIND_SQL);
    doc.diesel(R_BIND_RUST);
    let after: u64 = 2;
    let through: u64 = 5;
    let orm: Vec<(u64, String)> = events::table
        .filter(events::id.gt(bind(after)))
        .filter(events::id.le(bind(through)))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_BIND_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .filter(events::id.gt(bind(after)))
            .filter(events::id.le(bind(through)))
            .select((events::id, events::tenant_id))
            .order(events::id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: optional filter with when() ------------------------------
    doc.recipe(
        "Optional filters with `when(...)`",
        "On most backends an optional filter is added by boxing the query, but \
         the ClickHouse backend does not support `.into_boxed()`. `when(enabled, \
         predicate)` covers the gap: when `enabled` is true the predicate renders \
         normally; when it is false the node renders the always-true constant `1`, \
         so the filter contributes nothing and binds nothing. This replaces the \
         `(? = '' OR col = ?)` sentinel trick — the value is referenced once and \
         stays fully typed. (`when` renders different SQL per branch; if you need \
         the SQL *text* to stay identical across calls — for ClickHouse's query \
         cache or a parameterized view — see the named-parameter recipe next.)",
    );
    doc.sql(R_WHEN_SQL);
    doc.diesel(R_WHEN_RUST);
    // A real filter value arrives at runtime; an empty value disables the filter.
    let tenant = env::var("COOKBOOK_TENANT").unwrap_or_else(|_| "acme".to_owned());
    let tenant = tenant.as_str();
    let orm: Vec<(u64, String)> = events::table
        .filter(when(!tenant.is_empty(), events::tenant_id.eq(tenant)))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_WHEN_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .filter(when(!tenant.is_empty(), events::tenant_id.eq(tenant)))
            .select((events::id, events::tenant_id))
            .order(events::id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));
    doc.paragraph(
        "With an empty `tenant` the predicate disables itself — the same query \
         renders the filter as the constant `1`, so every row matches and no \
         value is bound:",
    );
    doc.code(
        "sql",
        &to_sql(
            &events::table
                .filter(when(false, events::tenant_id.eq("")))
                .select((events::id, events::tenant_id))
                .order(events::id.asc()),
        )?,
    );

    // ----- Recipe: named parameters for stable SQL --------------------------
    doc.recipe(
        "Stable SQL text with ClickHouse named parameters",
        "When the SQL string must stay byte-for-byte identical across calls — so \
         ClickHouse's query cache hits, or a parameterized view sees one query — \
         use a ClickHouse named parameter (`{name:Type}`). It can be referenced \
         many times in the text but is bound once with the client's `.param(...)`, \
         which makes it a tidy fit for the optional-filter sentinel: an empty value \
         leaves `'' = ''` true and disables the filter, a non-empty value enforces \
         it, and the SQL text never changes. The `{name:Type}` string is raw, so \
         its contents are unchecked; `to_sql_with_metadata(...).named_parameters()` \
         reports the names so a test can assert your `.param(...)` calls line up.",
    );
    doc.sql(R_NAMED_SQL);
    doc.diesel(R_NAMED_RUST);
    let named_query = events::table
        .select((events::id, events::tenant_id))
        .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
            "({tenant:String} = '' OR tenant_id = {tenant:String})",
        ))
        .order(events::id.asc());
    let rendered = to_sql_with_metadata(&named_query)?;
    assert_eq!(rendered.named_parameters(), &["tenant"]);
    doc.rendered(&rendered.sql);
    // One stable SQL string, bound by name. An empty value disables the filter.
    let enabled: Vec<(u64, String)> = client
        .query(&rendered.sql)
        .param("tenant", "acme")
        .fetch_all()
        .await?;
    let disabled: Vec<(u64, String)> = client
        .query(&rendered.sql)
        .param("tenant", "")
        .fetch_all()
        .await?;
    doc.text_output(&format!(
        "named parameters: {:?}\n\ntenant = \"acme\" -> {enabled:#?}\n\ntenant = \"\" matches all {} rows",
        rendered.named_parameters(),
        disabled.len(),
    ));

    // ----- Recipe: raw fragments with Diesel-owned binds --------------------
    doc.recipe(
        "Raw SQL fragments with Diesel-owned binds",
        "When a ClickHouse function has no typed helper yet, build the raw \
         fragment with Diesel's SQL literal binder: start with `sql::<T>(...)`, \
         call `.bind::<SqlType, _>(value)` at the exact point where the value \
         belongs, then continue with `.sql(...)`. Do not put a bare `?` inside \
         `sql::<T>(\"...\")` and bind it later through another client; that \
         reintroduces the ordering hazard this connection is meant to remove. \
         Literal question marks inside strings or comments are ignored by the \
         async connection's placeholder scanner.",
    );
    doc.sql(R_RAW_BIND_SQL);
    doc.diesel(R_RAW_BIND_RUST);
    let needle = "rust";
    let score_expr =
        diesel::dsl::sql::<diesel::sql_types::Float>("toFloat32(if(positionCaseInsensitive(text, ")
            .bind::<diesel::sql_types::Text, _>(needle)
            .sql(") > 0, 1, 0)) AS score");
    let match_filter =
        diesel::dsl::sql::<diesel::sql_types::Bool>("positionCaseInsensitive(text, ")
            .bind::<diesel::sql_types::Text, _>(needle)
            .sql(") > 0 AND text != '?' /* ? in comment */");
    let orm: Vec<(u64, String, f32)> = documents::table
        .filter(documents::tenant_id.eq("acme"))
        .filter(match_filter)
        .select((documents::id, documents::text, score_expr))
        .order(diesel::dsl::sql::<diesel::sql_types::Float>("score").desc())
        .then_order_by(documents::processed_at.desc())
        .then_order_by(documents::id.asc())
        .limit(10_i64)
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String, f32)> = client.query(R_RAW_BIND_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &documents::table
            .filter(documents::tenant_id.eq("acme"))
            .filter(
                diesel::dsl::sql::<diesel::sql_types::Bool>("positionCaseInsensitive(text, ")
                    .bind::<diesel::sql_types::Text, _>(needle)
                    .sql(") > 0 AND text != '?' /* ? in comment */"),
            )
            .select((
                documents::id,
                documents::text,
                diesel::dsl::sql::<diesel::sql_types::Float>(
                    "toFloat32(if(positionCaseInsensitive(text, ",
                )
                .bind::<diesel::sql_types::Text, _>(needle)
                .sql(") > 0, 1, 0)) AS score"),
            ))
            .order(diesel::dsl::sql::<diesel::sql_types::Float>("score").desc())
            .then_order_by(documents::processed_at.desc())
            .then_order_by(documents::id.asc())
            .limit(10_i64),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: FINAL + typed projection ---------------------------------
    doc.recipe(
        "`FINAL` with a typed projection",
        "Wrap a table source in `final_table(...)` to render `FROM table FINAL`. \
         Diesel's `table!` macro does not make bare columns selectable from a \
         custom source, so project them with `source_column(...)`, which keeps \
         each column's SQL type. Here `cookbook_latest` is a \
         `ReplacingMergeTree`, so `FINAL` collapses the duplicate `id = 1` to its \
         latest version.",
    );
    doc.sql(R_FINAL_SQL);
    doc.diesel(R_FINAL_RUST);
    let orm: Vec<(u64, String)> = final_table(latest::table)
        .select((source_column(latest::id), source_column(latest::status)))
        .order(source_column(latest::id).asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_FINAL_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &final_table(latest::table)
            .select((source_column(latest::id), source_column(latest::status)))
            .order(source_column(latest::id).asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: typed ClickHouse join ------------------------------------
    doc.recipe(
        "A ClickHouse join with a typed `.on(...)`",
        "`clickhouse_join(...)` renders ClickHouse's executable join grammar \
         (`ANY`/`ALL`/`ASOF`, `SEMI`/`ANTI`, `GLOBAL`). The `.on(...)` predicate \
         must be boolean and is written with real, type-checked columns; the \
         projection uses `source_column(...)` for the same reason as `FINAL` \
         above.",
    );
    doc.sql(R_JOIN_SQL);
    doc.diesel(R_JOIN_RUST);
    let orm: Vec<(u64, String)> = events::table
        .clickhouse_join(tenants::table)
        .any()
        .inner()
        .on(events::tenant_id.eq(tenants::tenant_id))
        .select((source_column(events::id), source_column(tenants::plan)))
        .order(source_column(events::id).asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_JOIN_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .clickhouse_join(tenants::table)
            .any()
            .inner()
            .on(events::tenant_id.eq(tenants::tenant_id))
            .select((source_column(events::id), source_column(tenants::plan)))
            .order(source_column(events::id).asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: aggregates with aliases ----------------------------------
    doc.recipe(
        "Aggregates with aliases: `count()`, `count_if`",
        "`count()` is ClickHouse's native row count (typed `UInt64`, loads into \
         `u64`); `count_if(predicate)` counts matching rows. `expr_as(expr, \
         \"alias\")` names any expression for struct-friendly result metadata.",
    );
    doc.sql(R_AGG_SQL);
    doc.diesel(R_AGG_RUST);
    // The Diesel side types `count_if` as `BigInt` (`i64`) while ClickHouse sends
    // `UInt64`; the parity check compares formatted values, so `2i64` and `2u64`
    // match. The raw side decodes both columns as the `u64` they are on the wire.
    let orm: Vec<(String, u64, i64)> = events::table
        .group_by(events::tenant_id)
        .select((
            events::tenant_id,
            expr_as(count(), "n"),
            expr_as(count_if(events::success), "ok"),
        ))
        .order(events::tenant_id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(String, u64, u64)> = client.query(R_AGG_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .group_by(events::tenant_id)
            .select((
                events::tenant_id,
                expr_as(count(), "n"),
                expr_as(count_if(events::success), "ok"),
            ))
            .order(events::tenant_id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: array functions in a predicate ---------------------------
    doc.recipe(
        "Array functions in a predicate",
        "ClickHouse array functions are typed bindings too. `has(array, value)` \
         tests membership and returns `Bool`, so it composes directly inside \
         `.filter(...)`.",
    );
    doc.sql(R_ARRAY_SQL);
    doc.diesel(R_ARRAY_RUST);
    let orm: Vec<(u64, String)> = events::table
        .filter(has(events::tags, "paid"))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_ARRAY_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .filter(has(events::tags, "paid"))
            .select((events::id, events::tenant_id))
            .order(events::id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: async array parameters -----------------------------------
    doc.recipe(
        "Array parameters through async execution",
        "`AsyncClickHouseConnection` can carry Diesel-collected array values, so \
         membership filters and higher-order array predicates do not need \
         external `.param(...)` calls. Use `bind::<Array<T>, _>(vec)` for a typed \
         array expression; `array_exists2(lambda2(...), left, right)` covers the \
         common parallel-array predicate shape.",
    );
    doc.sql(R_ARRAY_PARAM_SQL);
    doc.diesel(R_ARRAY_PARAM_RUST);
    type EventIds = diesel_clickhouse::sql_types::Array<diesel_clickhouse::sql_types::UInt64>;
    type TextArray = diesel_clickhouse::sql_types::Array<diesel::sql_types::Text>;
    let event_ids = vec![1_u64, 4_u64];
    let orm: Vec<(u64, String)> = events::table
        .filter(has(bind::<EventIds, _>(event_ids), events::id))
        .select((events::id, events::tenant_id))
        .order(events::id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, String)> = client.query(R_ARRAY_PARAM_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &events::table
            .filter(has(bind::<EventIds, _>(vec![1_u64, 4_u64]), events::id))
            .select((events::id, events::tenant_id))
            .order(events::id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    let allowed_pair_rows: Vec<u64> = events::table
        .filter(array_exists2(
            lambda2(
                "allowed_tenant",
                "allowed_status",
                "allowed_tenant = tenant_id AND allowed_status = if(success, 'ok', 'fail')",
            ),
            bind::<TextArray, _>(vec!["acme".to_owned(), "beta".to_owned()]),
            bind::<TextArray, _>(vec!["ok".to_owned(), "fail".to_owned()]),
        ))
        .select(events::id)
        .order(events::id.asc())
        .load(&mut conn)
        .await?;
    assert_eq!(allowed_pair_rows, vec![1, 3, 6]);
    doc.text_output(&format!(
        "parallel arrayExists allowed ids: {allowed_pair_rows:#?}"
    ));

    // ----- Recipe: vector search -------------------------------------------
    doc.recipe(
        "Vector scoring through async execution",
        "A query embedding is just an `Array(Float32)` bind. Diesel owns the \
         vector bytes, and `vector_dot_product_f32` hides the \
         ClickHouse-specific `arrayMap`/`arraySum` scoring expression.",
    );
    doc.sql(R_VECTOR_SQL);
    doc.diesel(R_VECTOR_RUST);
    type Float32Array = diesel_clickhouse::sql_types::Array<diesel::sql_types::Float>;
    let query_vector = vec![1.0_f32, 0.0_f32];
    let orm: Vec<(u64, f32)> = documents::table
        .filter(documents::tenant_id.eq("acme"))
        .select((
            documents::id,
            expr_as(
                vector_dot_product_f32(documents::embedding, bind::<Float32Array, _>(query_vector)),
                "score",
            ),
        ))
        .order(alias_ref::<diesel::sql_types::Float>("score").desc())
        .then_order_by(documents::id.asc())
        .load(&mut conn)
        .await?;
    let raw: Vec<(u64, f32)> = client.query(R_VECTOR_SQL).fetch_all().await?;
    doc.rendered(&to_sql(
        &documents::table
            .filter(documents::tenant_id.eq("acme"))
            .select((
                documents::id,
                expr_as(
                    vector_dot_product_f32(
                        documents::embedding,
                        bind::<Float32Array, _>(vec![1.0_f32, 0.0_f32]),
                    ),
                    "score",
                ),
            ))
            .order(alias_ref::<diesel::sql_types::Float>("score").desc())
            .then_order_by(documents::id.asc()),
    )?);
    doc.shared_output(&parity(&orm, &raw));

    // ----- Recipe: named struct loading -------------------------------------
    doc.recipe(
        "Named struct result mapping",
        "For typed projections, derive `Queryable` and keep the struct fields in \
         select-list order. For raw SQL or heavily aliased rows, derive \
         `QueryableByName` and annotate each field with the ClickHouse/Diesel SQL \
         type. Existing `clickhouse::Row` read structs can use \
         `load_clickhouse_rows(...)` as a migration bridge. All forms execute \
         through `AsyncClickHouseConnection`, so bind values still stay \
         Diesel-owned.",
    );
    doc.diesel(R_STRUCT_RUST);
    #[derive(Debug, PartialEq, Queryable)]
    struct TenantOverview {
        tenant_id: String,
        n: u64,
        ok: i64,
    }
    #[derive(Debug, PartialEq, QueryableByName)]
    struct DocumentHit {
        #[diesel(sql_type = diesel_clickhouse::sql_types::UInt64)]
        id: u64,
        #[diesel(sql_type = diesel::sql_types::Text)]
        text: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        source_type: Option<String>,
        #[diesel(sql_type = diesel_clickhouse::sql_types::Array<diesel::sql_types::Float>)]
        embedding: Vec<f32>,
    }
    #[derive(Debug, PartialEq, clickhouse::Row, serde::Deserialize)]
    struct DocumentHitRow {
        id: u64,
        text: String,
    }
    let overview: Vec<TenantOverview> = events::table
        .group_by(events::tenant_id)
        .select((
            events::tenant_id,
            expr_as(count(), "n"),
            expr_as(count_if(events::success), "ok"),
        ))
        .order(events::tenant_id.asc())
        .load(&mut conn)
        .await?;
    let hits: Vec<DocumentHit> = diesel::sql_query(
        "SELECT id, text, nullIf(source_type, '') AS source_type, embedding \
         FROM cookbook_documents WHERE tenant_id = ? ORDER BY id LIMIT 2",
    )
    .bind::<diesel::sql_types::Text, _>("acme")
    .load(&mut conn)
    .await?;
    let row_bridge_hits: Vec<DocumentHitRow> = conn
        .load_clickhouse_rows(
            documents::table
                .filter(documents::tenant_id.eq("acme"))
                .select((documents::id, documents::text))
                .order(documents::id.asc())
                .limit(2_i64),
        )
        .await?;
    assert_eq!(overview.len(), 2);
    assert_eq!(hits.len(), 2);
    assert_eq!(row_bridge_hits.len(), 2);
    doc.text_output(&format!(
        "typed aggregate rows: {overview:#?}\n\nraw/aliased document rows: {hits:#?}\n\nclickhouse::Row bridge rows: {row_bridge_hits:#?}"
    ));

    // ----- Recipe: render-only path -----------------------------------------
    doc.recipe(
        "Render SQL for another client with `to_sql_with_metadata`",
        "When you execute through a ClickHouse client other than \
         `AsyncClickHouseConnection`, render the query and inspect its \
         placeholders. `to_sql_with_metadata` reports the positional `?` count, \
         Diesel-collected positional bind types, and any named HTTP parameter \
         names/types/occurrence counts, so a test can assert your `.bind(...)` \
         and `.param(...)` calls line up. Remember: `to_sql` renders only the \
         SQL — the Diesel bind values stay behind, so you re-supply them in \
         render order.",
    );
    let rendered = to_sql_with_metadata(
        &events::table
            .filter(events::tenant_id.eq("acme"))
            .filter(events::success.eq(true))
            .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
                "has({allowed:Array(String)}, tags)",
            ))
            .select(events::id),
    )?;
    doc.diesel(R_METADATA_RUST);
    doc.rendered(&rendered.sql);
    doc.text_output(&format!(
        "positional `?` placeholders: {}\npositional bind types: {:?}\nnamed parameters: {:?}\nnamed parameter details: {:?}",
        rendered.positional_bind_count(),
        rendered.positional_bind_types(),
        rendered.named_parameters(),
        rendered.named_parameter_details(),
    ));

    // ----- Recipe: inserts --------------------------------------------------
    doc.recipe(
        "Single-row insert vs. batch ingest",
        "Single rows insert through Diesel's `insert_into(...).values(...)`. For \
         any real volume, use `insert_batch(...)`: one columnar RowBinary request \
         instead of N round-trips. (Multi-row inserts through Diesel's DSL are \
         not expressible on ClickHouse — see the usage guide.)",
    );
    doc.diesel(R_INSERT_RUST);
    let insert_sql = to_sql(
        &diesel::insert_into(tenants::table)
            .values((tenants::tenant_id.eq("delta"), tenants::plan.eq("free"))),
    )?;
    doc.rendered(&insert_sql);
    let written = conn
        .insert_batch(
            "cookbook_ingest",
            vec![
                IngestRow {
                    id: 100,
                    tenant_id: "acme".to_owned(),
                },
                IngestRow {
                    id: 101,
                    tenant_id: "beta".to_owned(),
                },
            ],
        )
        .await?;
    doc.text_output(&format!("insert_batch wrote {written} rows"));

    // ----- Recipe: when raw SQL is appropriate ------------------------------
    doc.recipe(
        "When raw SQL is the right call — and what you give up",
        "Reach for `sql::<T>(...)` only when ClickHouse's grammar has no typed \
         binding — for example an `ASOF` join whose `ON` mixes equality and \
         inequality across columns. The expression's result type (`<T>`) is still \
         checked, but its **contents are not**: column names, operators, and \
         table references inside the string are opaque to the compiler, so a typo \
         surfaces only at runtime. Keep these strings small and local.",
    );
    doc.sql(R_RAW_SQL);
    doc.diesel(R_RAW_RUST);
    let asof_sql = to_sql(
        &events::table
            .clickhouse_join(tenants::table)
            .asof()
            .left()
            .on(diesel::dsl::sql::<diesel::sql_types::Bool>(
                "`cookbook_events`.`tenant_id` = `cookbook_tenants`.`tenant_id`",
            ))
            .select(source_column(events::id)),
    )?;
    doc.rendered(&asof_sql);

    let markdown = doc.finish();
    if let Some(path) = output {
        fs::write(&path, markdown)?;
        eprintln!("wrote {path}");
    } else {
        print!("{markdown}");
    }

    Ok(())
}

/// Assert the ORM and raw-SQL results are identical and return the shared,
/// formatted rows for the document. Comparison is on the `Debug` rendering so
/// the two decode paths may land on different-but-equal numeric types
/// (e.g. `count_if` as `i64` vs ClickHouse `UInt64`).
fn parity<A: Debug, B: Debug>(orm: &A, raw: &B) -> String {
    let orm_fmt = format!("{orm:#?}");
    let raw_fmt = format!("{raw:#?}");
    assert_eq!(
        orm_fmt, raw_fmt,
        "cookbook parity check failed: the Diesel query and the raw SQL returned \
         different results.\nDiesel:\n{orm_fmt}\nraw SQL:\n{raw_fmt}"
    );
    orm_fmt
}

fn intro(doc: &mut CookbookDoc) {
    doc.heading(1, "Cookbook");
    doc.paragraph(
        "Copy-paste recipes for common ClickHouse queries, each shown as raw SQL \
         next to the equivalent Diesel query. Every recipe below was generated by \
         running **both** forms against a live ClickHouse server and asserting \
         they return identical rows — see `examples/cookbook.rs`. If a recipe's \
         two forms ever diverge, the document fails to build.",
    );
    doc.paragraph(
        "For the model behind these recipes (execution modes, the safety \
         trade-offs of `source_column`, the bind-value caveat), read the usage \
         guide. For a narrative walkthrough, read the tutorial.",
    );

    doc.heading(2, "Start here: choose your path");
    doc.unordered_list(&[
        "**Want Diesel to own the bind values?** Execute with \
         `AsyncClickHouseConnection` and `.load`/`.execute` — the recipes below \
         use this path.",
        "**Rendering SQL for another ClickHouse client?** Use \
         `to_sql_with_metadata` and re-supply the bind values yourself.",
        "**Need bulk ingestion?** Use `AsyncClickHouseConnection::insert_batch` — \
         one columnar request, not N inserts.",
        "**Need ClickHouse syntax with no typed binding?** Use `sql::<T>(...)`, \
         but know its contents are unchecked (last recipe).",
    ]);

    doc.heading(2, "SQL → API cheat sheet");
    doc.table(
        &["ClickHouse SQL", "diesel-clickhouse"],
        &[
            ["FROM t FINAL", "final_table(t::table)"],
            ["FROM t PREWHERE p", "prewhere(t::table, p)"],
            ["FROM t SAMPLE r", "sample(t::table, r)"],
            [
                "a ANY INNER JOIN b ON ...",
                ".clickhouse_join(b).any().inner().on(...)",
            ],
            ["count()", "count()"],
            ["countIf(p)", "count_if(p)"],
            ["any(col)", "any_value(col)"],
            ["quantile(0.95)(col)", "quantile(0.95, col)"],
            ["expr AS name", "expr_as(expr, \"name\")"],
            ["t.unsigned_col > 42", "t::unsigned_col.gt(bind(42u64))"],
            ["optional WHERE col = v", "when(flag, t::col.eq(v))"],
            [
                "ORDER BY score alias",
                "alias_ref::<Float>(\"score\").desc()",
            ],
            ["Array(UInt64) bind", "bind::<Array<UInt64>, _>(vec![...])"],
            [
                "raw function with a bind",
                "sql::<T>(\"fn(\").bind::<ST, _>(v).sql(\")\")",
            ],
            [
                "{name:Type} (stable SQL text)",
                "sql::<T>(\"{name:Type}\") + .param(..)",
            ],
            ["bare t.col from a custom source", "source_column(t::col)"],
            ["FORMAT JSONEachRow", ".format(Format::JsonEachRow)"],
        ],
    );

    doc.heading(2, "Regenerate this document");
    doc.paragraph(
        "Run the cookbook example against a live ClickHouse server. The `just` \
         recipe starts a disposable container, runs the example, writes this \
         file, and cleans up:",
    );
    doc.code("bash", "just cookbook");
    doc.paragraph("The `text` blocks below are the live results captured during that run.");

    doc.heading(2, "Recipes");
}

struct CookbookDoc {
    markdown: String,
}

impl CookbookDoc {
    fn new() -> Self {
        Self {
            markdown: String::new(),
        }
    }

    fn heading(&mut self, level: usize, title: &str) {
        self.markdown
            .push_str(&format!("\n{} {title}\n\n", "#".repeat(level)));
    }

    fn paragraph(&mut self, text: &str) {
        self.markdown.push_str(&collapse_ws(text));
        self.markdown.push_str("\n\n");
    }

    fn unordered_list(&mut self, items: &[&str]) {
        for item in items {
            self.markdown.push_str("- ");
            self.markdown.push_str(&collapse_ws(item));
            self.markdown.push('\n');
        }
        self.markdown.push('\n');
    }

    fn table(&mut self, headers: &[&str; 2], rows: &[[&str; 2]]) {
        self.markdown
            .push_str(&format!("| {} | {} |\n", headers[0], headers[1]));
        self.markdown.push_str("| --- | --- |\n");
        for row in rows {
            self.markdown
                .push_str(&format!("| `{}` | `{}` |\n", row[0], row[1]));
        }
        self.markdown.push('\n');
    }

    /// Start a recipe: a level-3 heading plus its description.
    fn recipe(&mut self, title: &str, description: &str) {
        self.heading(3, title);
        self.paragraph(description);
    }

    fn sql(&mut self, sql: &str) {
        self.paragraph("ClickHouse SQL:");
        self.code("sql", sql);
    }

    fn diesel(&mut self, rust: &str) {
        self.paragraph("Diesel:");
        // Narrative fragments that reference `conn` and the table modules and use
        // `.await` outside an `async fn`, so they are not standalone-compilable.
        // `rust,ignore` keeps `cargo test --doc` from building them, matching the
        // convention in docs/USAGE.md and docs/TUTORIAL.md.
        self.code("rust,ignore", rust);
    }

    fn rendered(&mut self, sql: &str) {
        self.paragraph("Rendered by `diesel-clickhouse`:");
        self.code("sql", sql);
    }

    fn shared_output(&mut self, output: &str) {
        self.paragraph("Both the ClickHouse SQL and the Diesel query above return the same rows:");
        self.code("text", output);
    }

    fn text_output(&mut self, output: &str) {
        self.paragraph("Output from this run:");
        self.code("text", output);
    }

    fn code(&mut self, language: &str, code: &str) {
        self.markdown.push_str("```");
        self.markdown.push_str(language);
        self.markdown.push('\n');
        self.markdown.push_str(code.trim());
        self.markdown.push_str("\n```\n\n");
    }

    fn finish(self) -> String {
        self.markdown.trim_start().to_owned()
    }
}

/// Collapse the soft-wrapped whitespace in a multi-line Rust string literal into
/// single spaces, so prose paragraphs render as one line in Markdown.
fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

const SETUP_SQL: &str = r#"
DROP TABLE IF EXISTS cookbook_events;
CREATE TABLE cookbook_events (
    id UInt64,
    tenant_id String,
    success Bool,
    latency_ms Float64,
    tags Array(String)
) ENGINE = MergeTree ORDER BY id;
INSERT INTO cookbook_events VALUES
    (1, 'acme', true, 10.0, ['paid', 'mobile']),
    (2, 'acme', false, 20.0, ['paid']),
    (3, 'acme', true, 30.0, ['trial']),
    (4, 'beta', true, 40.0, ['paid', 'desktop']),
    (5, 'beta', true, 50.0, ['mobile']),
    (6, 'beta', false, 60.0, ['trial']);

DROP TABLE IF EXISTS cookbook_tenants;
CREATE TABLE cookbook_tenants (tenant_id String, plan String) ENGINE = Memory;
INSERT INTO cookbook_tenants VALUES
    ('acme', 'enterprise'), ('beta', 'starter'), ('gamma', 'trial');

DROP TABLE IF EXISTS cookbook_latest;
CREATE TABLE cookbook_latest (id UInt64, status String, version UInt64)
    ENGINE = ReplacingMergeTree(version) ORDER BY id;
INSERT INTO cookbook_latest VALUES (1, 'open', 1), (1, 'closed', 2), (2, 'open', 1);

DROP TABLE IF EXISTS cookbook_documents;
CREATE TABLE cookbook_documents (
    id UInt64,
    tenant_id String,
    text String,
    source_type String,
    processed_at DateTime64(3),
    embedding Array(Float32)
) ENGINE = Memory;
INSERT INTO cookbook_documents VALUES
    (1, 'acme', 'Rust ? async guide', 'blog', '2024-01-01 00:00:00.000', [1.0, 0.0]),
    (2, 'acme', 'ClickHouse diesel notes', '', '2024-01-01 00:05:00.000', [0.2, 0.8]),
    (3, 'beta', 'Rust analytics', 'doc', '2024-01-01 00:10:00.000', [0.4, 0.6]),
    (4, 'acme', 'rust diesel clickhouse', 'doc', '2024-01-01 00:15:00.000', [0.9, 0.1]);

DROP TABLE IF EXISTS cookbook_ingest;
CREATE TABLE cookbook_ingest (id UInt64, tenant_id String) ENGINE = Memory;
"#;

const R_BOOTSTRAP_RUST: &str = r#"use diesel_async::SimpleAsyncConnection;
use diesel_clickhouse::{
    clickhouse::{self, sql::Identifier}, create_table, ClickHouseConnectionOptions,
    DataType, TableEngine, to_sql,
};

// Admin/bootstrap DDL uses the direct ClickHouse client without a database.
let admin = clickhouse::Client::default().with_url("http://localhost:8123");
admin.query("CREATE DATABASE IF NOT EXISTS ?")
    .bind(Identifier("analytics"))
    .execute().await?;

let ddl = create_table("analytics.events")
    .if_not_exists()
    .column("id", DataType::UInt64)
    .engine(TableEngine::memory());
admin.query(&to_sql(&ddl)?).execute().await?;

// Once the database exists, use the async Diesel connection for normal work.
let mut conn = ClickHouseConnectionOptions::new("http://localhost:8123")
    .database("analytics")
    .connect().await?;
conn.batch_execute("INSERT INTO events VALUES (1)").await?;"#;

const R_FILTER_SQL: &str = "SELECT id, tenant_id FROM cookbook_events WHERE tenant_id = 'acme' AND success = true ORDER BY id";

const R_FILTER_RUST: &str = r#"let rows: Vec<(u64, String)> = events::table
    .filter(events::tenant_id.eq("acme"))
    .filter(events::success.eq(true))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .load(&mut conn).await?;"#;

const R_MIGRATE_RUST: &str = r#"use diesel_async::RunQueryDsl;
use diesel_clickhouse::{to_sql_with_metadata, AsyncClickHouseConnection};

let query = events::table
    .filter(events::tenant_id.eq("acme"))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .limit(2_i64);

// Render-only/external-client path: tests can count placeholders, but the
// caller must still bind values in exactly the rendered order.
let rendered = to_sql_with_metadata(&query)?;
assert_eq!(rendered.positional_bind_count(), 2);
let rows = client
    .query(&rendered.sql)
    .bind("acme")
    .bind(2_i64)
    .fetch_all::<(u64, String)>()
    .await?;

// Preferred read path: Diesel collects and sends both binds for you.
let mut conn = AsyncClickHouseConnection::with_client(client.clone());
let rows: Vec<(u64, String)> = events::table
    .filter(events::tenant_id.eq("acme"))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .limit(2_i64)
    .load(&mut conn)
    .await?;"#;

const R_BIND_SQL: &str =
    "SELECT id, tenant_id FROM cookbook_events WHERE id > 2 AND id <= 5 ORDER BY id";

const R_BIND_RUST: &str = r#"use diesel_clickhouse::bind;

let after: u64 = 2;
let through: u64 = 5;
let rows: Vec<(u64, String)> = events::table
    // `events::id` is a `UInt64`; `bind` types each value against it.
    .filter(events::id.gt(bind(after)))
    .filter(events::id.le(bind(through)))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .load(&mut conn).await?;"#;

const R_WHEN_SQL: &str =
    "SELECT id, tenant_id FROM cookbook_events WHERE tenant_id = 'acme' ORDER BY id";

const R_WHEN_RUST: &str = r#"use diesel_clickhouse::when;

let tenant = "acme"; // an empty value disables the filter
let rows: Vec<(u64, String)> = events::table
    .filter(when(!tenant.is_empty(), events::tenant_id.eq(tenant)))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .load(&mut conn).await?;"#;

const R_NAMED_SQL: &str = "SELECT id, tenant_id FROM cookbook_events \
WHERE ({tenant:String} = '' OR tenant_id = {tenant:String}) ORDER BY id";

const R_NAMED_RUST: &str = r#"use diesel::dsl::sql;
use diesel::sql_types::Bool;
use diesel_clickhouse::to_sql_with_metadata;

// `{tenant:String}` appears twice in the text but is bound once, so the SQL
// string is identical for every tenant (and "" disables the filter).
let query = events::table
    .select((events::id, events::tenant_id))
    .filter(sql::<Bool>("({tenant:String} = '' OR tenant_id = {tenant:String})"))
    .order(events::id.asc());

let rendered = to_sql_with_metadata(&query)?;
assert_eq!(rendered.named_parameters(), &["tenant"]);

let rows: Vec<(u64, String)> = client
    .query(&rendered.sql)
    .param("tenant", "acme") // bind once by name; "" matches all rows
    .fetch_all().await?;"#;

const R_RAW_BIND_SQL: &str = "SELECT id, text, \
toFloat32(if(positionCaseInsensitive(text, 'rust') > 0, 1, 0)) AS score \
FROM cookbook_documents \
WHERE tenant_id = 'acme' AND positionCaseInsensitive(text, 'rust') > 0 \
ORDER BY score DESC, processed_at DESC, id ASC LIMIT 10";

const R_RAW_BIND_RUST: &str = r#"use diesel::dsl::sql;
use diesel::sql_types::{Bool, Float, Text};

let needle = "rust";
let score = sql::<Float>("toFloat32(if(positionCaseInsensitive(text, ")
    .bind::<Text, _>(needle)
    .sql(") > 0, 1, 0)) AS score");
let matches = sql::<Bool>("positionCaseInsensitive(text, ")
    .bind::<Text, _>(needle)
    .sql(") > 0 AND text != '?' /* ? in comment */");

let rows: Vec<(u64, String, f32)> = documents::table
    .filter(documents::tenant_id.eq("acme"))
    .filter(matches)
    .select((documents::id, documents::text, score))
    .order(sql::<Float>("score").desc())
    .then_order_by(documents::processed_at.desc())
    .then_order_by(documents::id.asc())
    .limit(10_i64)
    .load(&mut conn).await?;"#;

const R_FINAL_SQL: &str = "SELECT id, status FROM cookbook_latest FINAL ORDER BY id";

const R_FINAL_RUST: &str = r#"use diesel_clickhouse::{final_table, source_column};

let rows: Vec<(u64, String)> = final_table(latest::table)
    .select((source_column(latest::id), source_column(latest::status)))
    .order(source_column(latest::id).asc())
    .load(&mut conn).await?;"#;

const R_JOIN_SQL: &str = "SELECT cookbook_events.id, cookbook_tenants.plan \
FROM cookbook_events ANY INNER JOIN cookbook_tenants \
ON cookbook_events.tenant_id = cookbook_tenants.tenant_id \
ORDER BY cookbook_events.id";

const R_JOIN_RUST: &str = r#"use diesel_clickhouse::{ClickHouseJoinDsl, source_column};

let rows: Vec<(u64, String)> = events::table
    .clickhouse_join(tenants::table)
    .any()
    .inner()
    .on(events::tenant_id.eq(tenants::tenant_id))
    .select((source_column(events::id), source_column(tenants::plan)))
    .order(source_column(events::id).asc())
    .load(&mut conn).await?;"#;

const R_AGG_SQL: &str = "SELECT tenant_id, count() AS n, countIf(success) AS ok \
FROM cookbook_events GROUP BY tenant_id ORDER BY tenant_id";

const R_AGG_RUST: &str = r#"use diesel_clickhouse::{count, count_if, expr_as};

let rows: Vec<(String, u64, i64)> = events::table
    .group_by(events::tenant_id)
    .select((
        events::tenant_id,
        expr_as(count(), "n"),
        expr_as(count_if(events::success), "ok"),
    ))
    .order(events::tenant_id.asc())
    .load(&mut conn).await?;"#;

const R_ARRAY_SQL: &str =
    "SELECT id, tenant_id FROM cookbook_events WHERE has(tags, 'paid') ORDER BY id";

const R_ARRAY_RUST: &str = r#"use diesel_clickhouse::has;

let rows: Vec<(u64, String)> = events::table
    .filter(has(events::tags, "paid"))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .load(&mut conn).await?;"#;

const R_ARRAY_PARAM_SQL: &str = "SELECT id, tenant_id FROM cookbook_events \
WHERE has(CAST([1, 4], 'Array(UInt64)'), id) ORDER BY id";

const R_ARRAY_PARAM_RUST: &str = r#"use diesel::sql_types::Text;
use diesel_clickhouse::{array_exists2, bind, has, lambda2};
use diesel_clickhouse::sql_types::{Array, UInt64};

type EventIds = Array<UInt64>;
let rows: Vec<(u64, String)> = events::table
    .filter(has(bind::<EventIds, _>(vec![1_u64, 4_u64]), events::id))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
    .load(&mut conn).await?;

// Parallel string arrays for ClickHouse's higher-order `arrayExists` lambda.
type TextArray = Array<Text>;
let allowed_ids: Vec<u64> = events::table
    .filter(array_exists2(
        lambda2(
            "allowed_tenant",
            "allowed_status",
            "allowed_tenant = tenant_id AND allowed_status = if(success, 'ok', 'fail')",
        ),
        bind::<TextArray, _>(vec!["acme".to_owned(), "beta".to_owned()]),
        bind::<TextArray, _>(vec!["ok".to_owned(), "fail".to_owned()]),
    ))
    .select(events::id)
    .order(events::id.asc())
    .load(&mut conn).await?;"#;

const R_VECTOR_SQL: &str = "SELECT id, \
toFloat32(arraySum(arrayMap((x, y) -> x * y, embedding, [1.0, 0.0]))) AS score \
FROM cookbook_documents WHERE tenant_id = 'acme' ORDER BY score DESC, id ASC";

const R_VECTOR_RUST: &str = r#"use diesel::sql_types::Float;
use diesel_clickhouse::{alias_ref, bind, expr_as, vector_dot_product_f32};
use diesel_clickhouse::sql_types::Array;

type Float32Array = Array<Float>;
let query_vector = vec![1.0_f32, 0.0_f32];
let rows: Vec<(u64, f32)> = documents::table
    .filter(documents::tenant_id.eq("acme"))
    .select((
        documents::id,
        expr_as(
            vector_dot_product_f32(documents::embedding, bind::<Float32Array, _>(query_vector)),
            "score",
        ),
    ))
    .order(alias_ref::<Float>("score").desc())
    .then_order_by(documents::id.asc())
    .load(&mut conn).await?;"#;

const R_STRUCT_RUST: &str = r#"#[derive(Debug, Queryable)]
struct TenantOverview {
    tenant_id: String,
    n: u64,
    ok: i64,
}

let overview: Vec<TenantOverview> = events::table
    .group_by(events::tenant_id)
    .select((
        events::tenant_id,
        expr_as(count(), "n"),
        expr_as(count_if(events::success), "ok"),
    ))
    .order(events::tenant_id.asc())
    .load(&mut conn).await?;

#[derive(Debug, QueryableByName)]
struct DocumentHit {
    #[diesel(sql_type = diesel_clickhouse::sql_types::UInt64)]
    id: u64,
    #[diesel(sql_type = diesel::sql_types::Text)]
    text: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    source_type: Option<String>,
    #[diesel(sql_type = diesel_clickhouse::sql_types::Array<diesel::sql_types::Float>)]
    embedding: Vec<f32>,
}

let hits: Vec<DocumentHit> = diesel::sql_query(
    "SELECT id, text, nullIf(source_type, '') AS source_type, embedding \
     FROM cookbook_documents WHERE tenant_id = ? ORDER BY id LIMIT 2",
)
.bind::<diesel::sql_types::Text, _>("acme")
.load(&mut conn).await?;

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct DocumentHitRow {
    id: u64,
    text: String,
}

let row_bridge_hits: Vec<DocumentHitRow> = conn
    .load_clickhouse_rows(
        documents::table
            .filter(documents::tenant_id.eq("acme"))
            .select((documents::id, documents::text))
            .order(documents::id.asc())
            .limit(2_i64),
    )
    .await?;"#;

const R_METADATA_RUST: &str = r#"use diesel_clickhouse::to_sql_with_metadata;

let query = events::table
    .filter(events::tenant_id.eq("acme"))
    .filter(events::success.eq(true))
    .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
        "has({allowed:Array(String)}, tags)",
    ))
    .select(events::id);

let rendered = to_sql_with_metadata(&query)?;
// rendered.sql                         -> the SQL string, with `?` placeholders
// rendered.positional_bind_count()     -> how many values to bind, in order
// rendered.positional_bind_types()     -> ClickHouse type per Diesel bind
// rendered.named_parameters()          -> unique ClickHouse {name:Type} names
// rendered.named_parameter_details()   -> name/type/occurrence-count summaries"#;

const R_INSERT_RUST: &str = r#"// Single row, through Diesel:
diesel::insert_into(tenants::table)
    .values((tenants::tenant_id.eq("delta"), tenants::plan.eq("free")))
    .execute(&mut conn).await?;

// Many rows, one columnar request:
#[derive(clickhouse::Row, serde::Serialize)]
struct IngestRow { id: u64, tenant_id: String }

let written = conn.insert_batch("cookbook_ingest", vec![
    IngestRow { id: 100, tenant_id: "acme".to_owned() },
    IngestRow { id: 101, tenant_id: "beta".to_owned() },
]).await?;"#;

const R_RAW_SQL: &str = "SELECT id FROM cookbook_events \
ASOF LEFT JOIN cookbook_tenants \
ON cookbook_events.tenant_id = cookbook_tenants.tenant_id";

const R_RAW_RUST: &str = r#"use diesel::dsl::sql;
use diesel::sql_types::Bool;
use diesel_clickhouse::{ClickHouseJoinDsl, source_column};

// `ASOF ... ON` mixing `=` and `>=` has no typed binding, so the predicate is a
// checked-result-type but unchecked-contents `sql::<Bool>(...)` fragment.
let query = events::table
    .clickhouse_join(tenants::table)
    .asof()
    .left()
    .on(sql::<Bool>(
        "`cookbook_events`.`tenant_id` = `cookbook_tenants`.`tenant_id`",
    ))
    .select(source_column(events::id));"#;
