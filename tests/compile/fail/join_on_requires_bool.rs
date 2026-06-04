use diesel_clickhouse::ClickHouseJoinDsl;

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

    // `.on(...)` requires a boolean predicate. A bare non-boolean column (here a
    // `BigInt`) is rejected at compile time rather than rendering `ON <int>`.
    let _ = events
        .clickhouse_join(tenants::table)
        .inner()
        .on(id);
}
