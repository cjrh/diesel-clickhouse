use diesel::prelude::*;
use diesel_clickhouse::{ClickHouseJoinDsl, join_column, to_sql};

diesel::table! {
    events (id) {
        id -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    tenants (tenant_id) {
        tenant_id -> Text,
        plan -> Text,
    }
}

diesel::allow_tables_to_appear_in_same_query!(events, tenants);

// `join_column` makes table columns selectable from a `ClickHouseJoin` while
// preserving their SQL types, so the projection is type-checked rather than a
// hand-written `sql::<...>("...")` string.
fn assert_sql_type<Q>(_: &Q)
where
    Q: diesel::query_builder::Query<
            SqlType = (diesel::sql_types::BigInt, diesel::sql_types::Text),
        >,
{
}

fn main() {
    use self::events::dsl::*;

    let query = events
        .clickhouse_join(tenants::table)
        .any()
        .inner()
        .on(tenant_id.eq(tenants::tenant_id))
        .select((join_column(id), join_column(tenants::plan)));

    assert_sql_type(&query);
    let _ = to_sql(&query).unwrap();
}
