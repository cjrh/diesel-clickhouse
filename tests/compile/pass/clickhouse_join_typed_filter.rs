use diesel::prelude::*;
use diesel_clickhouse::{
    ClickHouseJoinDsl, count, expr_as, final_table, source_column, source_column_as, to_sql,
};

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    record_index (object_uri) {
        object_uri -> Text,
        tenant_id -> Text,
        generation_id -> BigInt,
        format -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    batches (tenant_id) {
        tenant_id -> Text,
        generation_id -> BigInt,
        status -> Text,
    }
}

diesel::allow_tables_to_appear_in_same_query!(record_index, batches);

// The whole "typed predicates on custom sources" shape compiles end to end:
// `FINAL` sources, a typed multi-column `.on(...)`, a type-checked projection
// mixing `source_column`/`expr_as`/`count()`, typed `.filter(...)` against
// columns from *both* sides of the join, and trailing `group_by`/`order` — all
// without a single `sql::<Bool>("...")` predicate or `sql::<...>("...")` select.
fn main() {
    let tenant_id = "acme";

    let query = final_table(record_index::table)
        .clickhouse_join(final_table(batches::table))
        .all()
        .inner()
        .on(record_index::tenant_id
            .eq(batches::tenant_id)
            .and(record_index::generation_id.eq(batches::generation_id)))
        .select((
            source_column(record_index::object_uri),
            source_column_as(record_index::format, "format"),
            expr_as(count(), "record_count"),
        ))
        .filter(record_index::tenant_id.eq(tenant_id))
        .filter(batches::tenant_id.eq(tenant_id))
        .filter(batches::status.eq("active"))
        .group_by(record_index::object_uri)
        .order(record_index::object_uri.asc());

    let _ = to_sql(&query).unwrap();
}
