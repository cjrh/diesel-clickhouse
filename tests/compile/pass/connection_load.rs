use std::collections::BTreeMap;

use diesel::prelude::*;
// Explicit import shadows the `RunQueryDsl` from diesel's prelude glob so the
// async connection's `.load`/`.first`/`.get_result` resolve unambiguously.
use diesel_async::RunQueryDsl;
use diesel_clickhouse::{AsyncClickHouseConnection, ClickHouseConnectionOptions};

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    users (id) {
        id -> BigInt,
        name -> Text,
        active -> Bool,
        created_at -> Timestamp,
        tags -> Array<Text>,
        attrs -> Map<Text, Text>,
    }
}

fn assert_async_connection<
    C: diesel_async::AsyncConnection<Backend = diesel_clickhouse::ClickHouse>,
>() {
}

fn main() {
    assert_async_connection::<AsyncClickHouseConnection>();
    let _options = ClickHouseConnectionOptions::new("http://localhost:8123")
        .user("default")
        .password("password")
        .database("analytics")
        .option("max_threads", "1");

    async fn loads_with_idiomatic_diesel(
        conn: &mut AsyncClickHouseConnection,
    ) -> diesel::QueryResult<()> {
        use self::users::dsl::*;

        let rows: Vec<(i64, String)> = users
            .filter(active.eq(true).and(name.eq("Tess")))
            .select((id, name))
            .order(id.asc())
            .load(conn)
            .await?;
        let _optional_name: Option<String> = users
            .select(name)
            .filter(id.eq(1_i64))
            .first(conn)
            .await
            .optional()?;
        let _tags: Vec<String> = users.select(tags).first(conn).await?;
        let _attrs: BTreeMap<String, String> = users.select(attrs).first(conn).await?;
        let _created_at: String = users.select(created_at).first(conn).await?;
        let _tuple: (String, i64) = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Tuple<(diesel::sql_types::Text, diesel::sql_types::BigInt)>,
        >("tuple('Tess', toInt64(1))"))
        .get_result(conn)
        .await?;
        let _tuple_array: Vec<(String, i64)> = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Array<diesel_clickhouse::sql_types::Tuple<(
                diesel::sql_types::Text,
                diesel::sql_types::BigInt,
            )>>,
        >("[('Tess', toInt64(1))]"))
        .get_result(conn)
        .await?;
        let _decimal: String = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Decimal64<2>,
        >("toDecimal64(123.45, 2)"))
        .get_result(conn)
        .await?;
        let _nullable_tags: Vec<Option<String>> = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Array<diesel::sql_types::Nullable<diesel::sql_types::Text>>,
        >("[toNullable('Tess'), NULL]"))
        .get_result(conn)
        .await?;
        let _nullable_attrs: BTreeMap<String, Option<i32>> = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Map<
                diesel::sql_types::Text,
                diesel::sql_types::Nullable<diesel::sql_types::Integer>,
            >,
        >("map('present', toNullable(toInt32(1)), 'missing', CAST(NULL, 'Nullable(Int32)'))"))
        .get_result(conn)
        .await?;
        let _nullable_tuple: (String, Option<i64>) = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Tuple<(
                diesel::sql_types::Text,
                diesel::sql_types::Nullable<diesel::sql_types::BigInt>,
            )>,
        >("tuple('Tess', CAST(NULL, 'Nullable(Int64)'))"))
        .get_result(conn)
        .await?;
        let _dynamic: String = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Dynamic,
        >("CAST(toUInt64(42), 'Dynamic')"))
        .get_result(conn)
        .await?;
        let _variant: String = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Variant<(
                diesel_clickhouse::sql_types::UInt64,
                diesel::sql_types::Text,
            )>,
        >("CAST(toUInt64(42), 'Variant(UInt64, String)')"))
        .get_result(conn)
        .await?;
        let _ = rows;
        Ok(())
    }

    let _ = loads_with_idiomatic_diesel;
}
