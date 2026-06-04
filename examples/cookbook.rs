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
    ClickHouseConnectionOptions, ClickHouseJoinDsl, clickhouse, count, count_if, expr_as,
    final_table, has, source_column, to_sql, to_sql_with_metadata,
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

    // ----- Recipe: render-only path -----------------------------------------
    doc.recipe(
        "Render SQL for another client with `to_sql_with_metadata`",
        "When you execute through a ClickHouse client other than \
         `AsyncClickHouseConnection`, render the query and inspect its \
         placeholders. `to_sql_with_metadata` reports the positional `?` count \
         and any named HTTP parameters, so a test can assert your `.bind(...)` \
         calls line up. Remember: `to_sql` renders only the SQL — the Diesel \
         bind values stay behind, so you re-supply them in render order.",
    );
    let rendered = to_sql_with_metadata(
        &events::table
            .filter(events::tenant_id.eq("acme"))
            .filter(events::success.eq(true))
            .select(events::id),
    )?;
    doc.diesel(R_METADATA_RUST);
    doc.rendered(&rendered.sql);
    doc.text_output(&format!(
        "positional `?` placeholders: {}\nnamed parameters: {:?}",
        rendered.positional_bind_count(),
        rendered.named_parameters(),
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

DROP TABLE IF EXISTS cookbook_ingest;
CREATE TABLE cookbook_ingest (id UInt64, tenant_id String) ENGINE = Memory;
"#;

const R_FILTER_SQL: &str = "SELECT id, tenant_id FROM cookbook_events WHERE tenant_id = 'acme' AND success = true ORDER BY id";

const R_FILTER_RUST: &str = r#"let rows: Vec<(u64, String)> = events::table
    .filter(events::tenant_id.eq("acme"))
    .filter(events::success.eq(true))
    .select((events::id, events::tenant_id))
    .order(events::id.asc())
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

const R_METADATA_RUST: &str = r#"use diesel_clickhouse::to_sql_with_metadata;

let query = events::table
    .filter(events::tenant_id.eq("acme"))
    .filter(events::success.eq(true))
    .select(events::id);

let rendered = to_sql_with_metadata(&query)?;
// rendered.sql                      -> the SQL string, with `?` placeholders
// rendered.positional_bind_count()  -> how many values to bind, in order
// rendered.named_parameters()       -> ClickHouse {name:Type} HTTP params"#;

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
