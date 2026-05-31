use diesel::prelude::*;
use diesel_clickhouse::{
    aggregate, create_table, DataType, NestedField, TableEngine, ClickHouseTextExpressionMethods,
};

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    events (id) {
        id -> BigInt,
        tenant_id -> Text,
        success -> Bool,
        latency_ms -> Double,
        payload -> Text,
    }
}

fn main() {
    use self::events::dsl::*;

    let _ddl = create_table("events")
        .column("id", DataType::UInt64)
        .column("tenant_id", DataType::low_cardinality(DataType::String))
        .column("location", DataType::Point)
        .column("flex", DataType::dynamic_with_max_types(4))
        .column(
            "variant_value",
            DataType::variant([DataType::UInt64, DataType::String]),
        )
        .column(
            "attrs",
            DataType::nested([
                NestedField::new("key", DataType::String),
                NestedField::new("value", DataType::String),
            ]),
        )
        .engine(TableEngine::memory());

    let search_query = events.select(tenant_id.ilike("%acme%"));
    let aggregate_query = events.select(
        aggregate::<diesel::sql_types::Double>("avg")
            .arg(latency_ms)
            .or_null()
            .if_(success),
    );
    let _ = diesel_clickhouse::to_sql(&search_query).unwrap();
    let _ = diesel_clickhouse::to_sql(&aggregate_query).unwrap();
}
