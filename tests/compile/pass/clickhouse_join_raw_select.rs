use diesel::prelude::*;
use diesel_clickhouse::{to_sql, ClickHouseJoinDsl};

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

fn main() {
    use self::events::dsl::*;

    let join_query = events
        .clickhouse_join(tenants::table)
        .global()
        .any()
        .inner()
        .using(["tenant_id"])
        .select(diesel::dsl::sql::<(
            diesel::sql_types::BigInt,
            diesel::sql_types::Text,
        )>("`events`.`id`, `tenants`.`plan`"));

    let _ = to_sql(&join_query).unwrap();
}
