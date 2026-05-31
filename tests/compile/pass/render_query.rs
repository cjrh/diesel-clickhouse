use diesel::prelude::*;
use diesel_clickhouse::{count_if, quantile, to_sql, ClickHouseQueryDsl, Format};

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    events (id) {
        id -> BigInt,
        tenant_id -> Text,
        success -> Bool,
        latency_ms -> Double,
        tags -> Array<Text>,
    }
}

fn main() {
    use self::events::dsl::*;

    let query = events
        .filter(tenant_id.eq("acme"))
        .group_by(tenant_id)
        .select((tenant_id, count_if(success), quantile(0.95, latency_ms)))
        .limit_by_col(10, "tenant_id")
        .format(Format::JsonEachRow);

    let _sql = to_sql(&query).unwrap();
}
