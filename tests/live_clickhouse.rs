//! End-to-end verification against a real ClickHouse server.
//!
//! The test starts `clickhouse/clickhouse-server` via testcontainers, creates a
//! scratch MergeTree table, renders queries through this crate's Diesel DSL, and
//! executes those rendered queries through the official `clickhouse` Rust client.
//! The container is removed automatically when the test finishes.
//!
//! Kept ignored so normal `cargo test` does not require Docker or pull a server
//! image. Run it explicitly with:
//!
//!     cargo test --test live_clickhouse -- --ignored --nocapture

use std::error::Error;
use std::time::Duration;

use diesel::prelude::*;
// Bring diesel-async's `RunQueryDsl` into scope explicitly. An explicit import
// shadows the `RunQueryDsl` pulled in by diesel's prelude glob, so `.load`/
// `.execute`/... resolve unambiguously to the async connection's methods.
use diesel_async::RunQueryDsl;
use testcontainers_modules::{
    clickhouse::{CLICKHOUSE_PORT, ClickHouse as ClickHouseImage},
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

use diesel_clickhouse::{
    AsyncClickHouseConnection, ClickHouseConnectionOptions, ClickHouseJoinDsl, ClickHouseQueryDsl,
    ClickHouseTextExpressionMethods, Column, DataType, InsertBatchOptions, NestedField, OverDsl,
    Setting, TableEngine, TableIndex, abs, accurate_cast_or_null, aggregate,
    aggregating_merge_tree, alias_ref, alter_table, analysis_of_variance, approx_top_sum,
    array_count, array_exists, array_exists2, array_filter, array_map, base64_decode,
    base64_encode, cast, ceil, city_hash64, concat, corr, count, count_if, count_merge, covar_pop,
    covar_pop_stable, covar_samp, covar_samp_stable, create_materialized_view, create_table,
    cut_query_string, date_diff, dense_rank, domain, domain_without_www, expr_as,
    farm_fingerprint64, final_table, finalize_aggregation, first_significant_subdomain, floor,
    greatest, group_by_all, grouping_sets, hex, histogram, ilike, ipv4_num_to_string,
    ipv4_string_to_num, ipv6_num_to_string, is_ipv4_string, is_ipv6_string, is_null, is_valid_json,
    join_column, json_extract_int, json_extract_int_path, json_extract_string_path, json_has,
    json_length, json_value, l2_distance, lag_in_frame, lambda, lambda2, least, left_utf8, length,
    length_utf8, lower, mann_whitney_u_test, map_apply, map_contains, map_filter, max_if,
    merge_tree, min_if, multi_match_any, multi_match_any_index, mutation_assignment, named_param,
    null_if, partition_by, partition_expr, position, position_case_insensitive, prewhere,
    projection, quantile, quantile_deterministic, quantile_exact, quantile_timing, quantiles,
    quantiles_timing, rank, regexp_match, replace_all, replacing_merge_tree, rollup, round,
    row_number, sample_offset, simple_json_extract_int, simple_json_extract_string,
    simple_json_has, sip_hash64, source_column, stddev_pop, stddev_pop_stable, stddev_samp,
    substring, sum_merge, sum_state, summing_merge_tree, to_date_time, to_float64,
    to_float64_or_null, to_int32, to_int32_or_null, to_int64, to_ipv4, to_ipv6, to_sql, to_string,
    to_uint64, to_uint64_or_null, top_k, top_level_domain, try_base64_decode, unhex, uniq_exact_if,
    uniq_exact_merge, upper, url_fragment, url_path, url_path_full, url_protocol, url_query_string,
    var_pop, var_pop_stable, vector_dot_product_f32, vector_f32, with_fill, xx_hash64,
};

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "diesel_clickhouse_events"]
    events (id) {
        id -> BigInt,
        tenant_id -> Text,
        created_at -> Timestamp,
        success -> Bool,
        latency_ms -> Double,
        tags -> Array<Text>,
        attrs -> Map<Text, Text>,
        payload -> Text,
    }
}

diesel::table! {
    #[sql_name = "diesel_clickhouse_tenants"]
    tenants (tenant_id) {
        tenant_id -> Text,
        plan -> Text,
    }
}

diesel::table! {
    #[sql_name = "diesel_clickhouse_tenant_rates"]
    tenant_rates (tenant_id, effective_at) {
        tenant_id -> Text,
        effective_at -> Timestamp,
        rate -> Double,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "diesel_clickhouse_aggregate_states"]
    aggregate_states (state_key) {
        state_key -> UInt64,
        latency_sum -> AggregateFunction<Double>,
        event_count -> AggregateFunction<BigInt>,
        exact_ids -> AggregateFunction<BigInt>,
    }
}

diesel::table! {
    #[sql_name = "diesel_clickhouse_mv_source"]
    mv_source (id) {
        id -> BigInt,
        tenant_id -> Text,
        success -> Bool,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "diesel_clickhouse_image_vectors"]
    image_vectors (id) {
        id -> BigInt,
        caption -> Text,
        embedding -> Array<Float>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "diesel_clickhouse_gold_documents"]
    gold_documents (id) {
        id -> UInt32,
        tenant_id -> Text,
        text -> Text,
        source_type -> Text,
        processed_at -> DateTime64,
        embedding -> Array<Float>,
    }
}

diesel::table! {
    #[sql_name = "diesel_clickhouse_connection_inserts"]
    connection_inserts (id) {
        id -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    #[sql_name = "diesel_clickhouse_connection_batch"]
    connection_batch (id) {
        id -> BigInt,
        tenant_id -> Text,
    }
}

// Row type for the `insert_batch` RowBinary path: a `clickhouse::Row` +
// `serde::Serialize` struct whose fields name and type the target columns.
#[derive(clickhouse::Row, serde::Serialize)]
struct BatchRow {
    id: i64,
    tenant_id: String,
}

diesel::joinable!(events -> tenants (tenant_id));
diesel::allow_tables_to_appear_in_same_query!(
    events,
    tenants,
    tenant_rates,
    aggregate_states,
    mv_source,
    image_vectors,
    gold_documents
);

// Single-row inserts through Diesel require `treat_none_as_default_value =
// false` on ClickHouse (no SQL `DEFAULT` keyword for `INSERT`).
#[derive(Insertable)]
#[diesel(table_name = connection_inserts, treat_none_as_default_value = false)]
struct NewConnectionInsert<'a> {
    id: i64,
    tenant_id: &'a str,
}

struct ClickHouseFixture {
    _node: ContainerAsync<ClickHouseImage>,
    url: String,
    client: clickhouse::Client,
}

async fn start_clickhouse() -> TestResult<ClickHouseFixture> {
    let node = ClickHouseImage::default()
        .with_tag("24.8-alpine")
        .with_env_var("CLICKHOUSE_USER", "default")
        .with_env_var("CLICKHOUSE_PASSWORD", "password")
        .start()
        .await?;
    let host = node.get_host().await?;
    let port = node.get_host_port_ipv4(CLICKHOUSE_PORT).await?;
    let url = format!("http://{host}:{port}");
    // These tests drive the raw `clickhouse` client to confirm that SQL rendered
    // by this crate *executes and returns the expected values*, not that the
    // assertion tuples' Rust types exactly match the ClickHouse column types.
    // clickhouse 0.14+ defaults to `RowBinaryWithNamesAndTypes`, which strictly
    // rejects benign reads like `UInt64` into `i64` or `UInt8` into `bool`.
    // Disabling validation uses positional `RowBinary` and keeps these
    // value-focused assertions lenient. (The `AsyncClickHouseConnection` path is
    // unaffected: it decodes text via `fetch_bytes`, not this serde path.)
    let client = clickhouse::Client::default()
        .with_url(&url)
        .with_user("default")
        .with_password("password")
        .with_validation(false);
    Ok(ClickHouseFixture {
        _node: node,
        url,
        client,
    })
}

async fn setup(client: &clickhouse::Client) -> TestResult<()> {
    let statements = [
        "DROP TABLE IF EXISTS diesel_clickhouse_events",
        "DROP TABLE IF EXISTS diesel_clickhouse_tenants",
        "DROP TABLE IF EXISTS diesel_clickhouse_tenant_rates",
        "DROP TABLE IF EXISTS diesel_clickhouse_aggregate_states",
        "DROP TABLE IF EXISTS diesel_clickhouse_mv",
        "DROP TABLE IF EXISTS diesel_clickhouse_mv_target",
        "DROP TABLE IF EXISTS diesel_clickhouse_mv_source",
        "DROP TABLE IF EXISTS diesel_clickhouse_image_vectors",
        "DROP TABLE IF EXISTS diesel_clickhouse_gold_documents",
        "DROP TABLE IF EXISTS diesel_clickhouse_type_showcase",
        "DROP TABLE IF EXISTS diesel_clickhouse_projection_rollups",
        "DROP TABLE IF EXISTS diesel_clickhouse_summing_rollups",
        "DROP TABLE IF EXISTS diesel_clickhouse_mutations",
        "DROP TABLE IF EXISTS diesel_clickhouse_partitions",
        "CREATE TABLE diesel_clickhouse_events (
            id UInt64,
            tenant_id String,
            created_at DateTime,
            success Bool,
            latency_ms Float64,
            tags Array(String),
            attrs Map(String, String),
            payload String
        ) ENGINE = ReplacingMergeTree
        ORDER BY (tenant_id, id)
        SAMPLE BY id",
        r#"INSERT INTO diesel_clickhouse_events VALUES
            (1, 'acme', '2024-01-01 00:10:00', true, 10.0, ['paid', 'mobile'], map('country', 'US', 'plan', 'pro'), '{"score": 10, "country": "US"}'),
            (2, 'acme', '2024-01-01 00:20:00', false, 20.0, ['paid'], map('country', 'US', 'plan', 'free'), '{"score": 20, "country": "US"}'),
            (3, 'acme', '2024-01-01 01:05:00', true, 30.0, ['trial'], map('country', 'CA', 'plan', 'pro'), '{"score": 30, "country": "CA"}'),
            (4, 'beta', '2024-01-01 00:15:00', true, 40.0, ['paid', 'desktop'], map('country', 'DE', 'plan', 'pro'), '{"score": 40, "country": "DE"}'),
            (5, 'beta', '2024-01-01 00:25:00', true, 50.0, ['mobile'], map('country', 'DE', 'plan', 'free'), '{"score": 50, "country": "DE"}'),
            (6, 'beta', '2024-01-01 01:35:00', false, 60.0, ['trial'], map('country', 'FR', 'plan', 'free'), '{"score": 60, "country": "FR"}')"#,
        "CREATE TABLE diesel_clickhouse_tenants (
            tenant_id String,
            plan String
        ) ENGINE = Memory",
        "INSERT INTO diesel_clickhouse_tenants VALUES
            ('acme', 'enterprise'),
            ('beta', 'starter'),
            ('gamma', 'trial')",
        "CREATE TABLE diesel_clickhouse_tenant_rates (
            tenant_id String,
            effective_at DateTime,
            rate Float64
        ) ENGINE = MergeTree
        ORDER BY (tenant_id, effective_at)",
        "INSERT INTO diesel_clickhouse_tenant_rates VALUES
            ('acme', '2024-01-01 00:00:00', 1.0),
            ('acme', '2024-01-01 01:00:00', 2.0),
            ('beta', '2024-01-01 00:00:00', 3.0)",
    ];

    for statement in statements {
        client.query(statement).execute().await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker; starts a real ClickHouse container"]
async fn full_dsl_battery_against_live_clickhouse() -> TestResult<()> {
    use self::events::dsl::*;

    let fixture = start_clickhouse().await?;
    setup(&fixture.client).await?;

    let native_sql = to_sql(
        &events
            .filter(
                tenant_id
                    .eq("acme")
                    .and(success.eq(true).or(latency_ms.gt(25.0))),
            )
            .group_by(tenant_id)
            .having(diesel::dsl::count_star().gt(1_i64))
            .select((tenant_id, diesel::dsl::count_star()))
            .order(tenant_id.asc())
            .limit(1)
            .offset(0),
    )?;
    let native_rows: Vec<(String, u64)> = fixture
        .client
        .query(&native_sql)
        .bind("acme")
        .bind(true)
        .bind(25.0)
        .bind(1_i64)
        .bind(1_i64)
        .bind(0_i64)
        .fetch_all()
        .await?;
    assert_eq!(native_rows, vec![("acme".to_string(), 2)]);

    let nullable_sql = to_sql(&diesel::select((
        diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Integer>>("NULL")
            .is_null(),
        diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Integer>>("NULL")
            .is_not_null(),
    )))?;
    let nullable_row: (bool, bool) = fixture.client.query(&nullable_sql).fetch_one().await?;
    assert_eq!(nullable_row, (true, false));

    let diesel_url = fixture
        .url
        .replacen("http://", "http://default:password@", 1);
    {
        use std::collections::BTreeMap;

        use diesel_async::{AsyncConnection, SimpleAsyncConnection};

        #[derive(Debug, PartialEq, QueryableByName)]
        struct ConnectionRow {
            #[diesel(column_name = id)]
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            row_id: i64,
            #[diesel(column_name = tenant_id)]
            #[diesel(sql_type = diesel::sql_types::Text)]
            row_tenant_id: String,
        }

        #[derive(Debug, PartialEq, QueryableByName)]
        struct ConnectionNetworkRow {
            #[diesel(column_name = uuid_value)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::Uuid)]
            row_uuid: String,
            #[diesel(column_name = ipv4_value)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::IPv4)]
            row_ipv4: String,
            #[diesel(column_name = ipv6_value)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::IPv6)]
            row_ipv6: String,
        }

        #[derive(Debug, PartialEq, QueryableByName)]
        struct ConnectionNullableCompositeRow {
            #[diesel(column_name = nullable_tags)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::Array<diesel::sql_types::Nullable<diesel::sql_types::Text>>)]
            row_nullable_tags: Vec<Option<String>>,
            #[diesel(column_name = nullable_attrs)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::Map<diesel::sql_types::Text, diesel::sql_types::Nullable<diesel::sql_types::Integer>>)]
            row_nullable_attrs: BTreeMap<String, Option<i32>>,
            #[diesel(column_name = nullable_tuple)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::Tuple<(diesel::sql_types::Text, diesel::sql_types::Nullable<diesel::sql_types::BigInt>)>)]
            row_nullable_tuple: (String, Option<i64>),
        }

        #[derive(Debug, PartialEq, QueryableByName)]
        struct ConnectionSemiStructuredRow {
            #[diesel(column_name = dynamic_value)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::Dynamic)]
            row_dynamic: String,
            #[diesel(column_name = variant_value)]
            #[diesel(sql_type = diesel_clickhouse::sql_types::Variant<(diesel_clickhouse::sql_types::UInt64, diesel::sql_types::Text)>)]
            row_variant: String,
        }

        #[derive(Debug, PartialEq, clickhouse::Row, serde::Deserialize)]
        struct ClickHouseDocumentScore {
            id: u32,
            text: String,
            score: f32,
        }

        #[derive(Debug, PartialEq, QueryableByName)]
        struct ConnectionWideNamedRow {
            #[diesel(sql_type = diesel_clickhouse::sql_types::UInt64)]
            id64: u64,
            #[diesel(sql_type = diesel_clickhouse::sql_types::UInt32)]
            id32: u32,
            #[diesel(column_name = tenant_id)]
            #[diesel(sql_type = diesel::sql_types::Text)]
            row_tenant_id: String,
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Integer>)]
            maybe_rank: Option<i32>,
            #[diesel(sql_type = diesel::sql_types::Float)]
            score: f32,
            #[diesel(sql_type = diesel_clickhouse::sql_types::Uuid)]
            uuid_value: String,
            #[diesel(sql_type = diesel_clickhouse::sql_types::DateTime64)]
            processed_at: String,
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
            account_id: Option<String>,
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
            source_type: Option<String>,
            #[diesel(sql_type = diesel_clickhouse::sql_types::UInt64)]
            count_value: u64,
            #[diesel(sql_type = diesel_clickhouse::sql_types::UInt32)]
            status_code: u32,
            #[diesel(sql_type = diesel::sql_types::Float)]
            metric_a: f32,
            #[diesel(sql_type = diesel::sql_types::Integer)]
            delta: i32,
            #[diesel(sql_type = diesel::sql_types::Text)]
            theme: String,
            #[diesel(sql_type = diesel::sql_types::Text)]
            sub_theme: String,
            #[diesel(sql_type = diesel_clickhouse::sql_types::Array<diesel::sql_types::Float>)]
            embedding: Vec<f32>,
        }

        let mut conn = AsyncClickHouseConnection::establish(&diesel_url).await?;
        let mut options_conn = ClickHouseConnectionOptions::from_url(&diesel_url)?
            .option("max_threads", "1")
            .connect()
            .await?;
        let options_value: i32 =
            diesel::select(diesel::dsl::sql::<diesel::sql_types::Integer>("toInt32(1)"))
                .get_result(&mut options_conn)
                .await?;
        assert_eq!(options_value, 1);
        let rows: Vec<(String, i64)> = events
            .filter(tenant_id.eq("acme").and(success.eq(true)))
            .group_by(tenant_id)
            .select((tenant_id, diesel::dsl::count_star()))
            .load(&mut conn)
            .await?;
        assert_eq!(rows, vec![("acme".to_string(), 2)]);

        let tag_values: Vec<String> = events
            .filter(id.eq(1_i64))
            .select(tags)
            .first(&mut conn)
            .await?;
        assert_eq!(tag_values, vec!["paid".to_string(), "mobile".to_string()]);

        let attrs_map: BTreeMap<String, String> = events
            .filter(id.eq(1_i64))
            .select(attrs)
            .first(&mut conn)
            .await?;
        assert_eq!(
            attrs_map,
            BTreeMap::from([
                ("country".to_string(), "US".to_string()),
                ("plan".to_string(), "pro".to_string()),
            ])
        );

        let created_at_value: String = events
            .filter(id.eq(1_i64))
            .select(created_at)
            .first(&mut conn)
            .await?;
        assert_eq!(created_at_value, "2024-01-01 00:10:00");

        let network_row = diesel::sql_query(
            "SELECT toUUID('550e8400-e29b-41d4-a716-446655440000') AS uuid_value, toIPv4('192.0.2.1') AS ipv4_value, toIPv6('2001:db8::1') AS ipv6_value",
        )
        .get_result::<ConnectionNetworkRow>(&mut conn).await?;
        assert_eq!(
            network_row,
            ConnectionNetworkRow {
                row_uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                row_ipv4: "192.0.2.1".to_string(),
                row_ipv6: "2001:db8::1".to_string(),
            }
        );

        let wide_row = diesel::sql_query(
            "SELECT \
             toUInt64(42) AS id64, \
             toUInt32(7) AS id32, \
             'acme' AS tenant_id, \
             CAST(NULL, 'Nullable(Int32)') AS maybe_rank, \
             toFloat32(0.75) AS score, \
             toUUID('550e8400-e29b-41d4-a716-446655440000') AS uuid_value, \
             toDateTime64('2024-01-02 03:04:05.123', 3) AS processed_at, \
             toNullable('account-1') AS account_id, \
             CAST(NULL, 'Nullable(String)') AS source_type, \
             toUInt64(99) AS count_value, \
             toUInt32(2) AS status_code, \
             toFloat32(1.5) AS metric_a, \
             toInt32(-4) AS delta, \
             'theme' AS theme, \
             'sub-theme' AS sub_theme, \
             [toFloat32(1), toFloat32(0.5)] AS embedding",
        )
        .get_result::<ConnectionWideNamedRow>(&mut conn)
        .await?;
        assert_eq!(
            wide_row,
            ConnectionWideNamedRow {
                id64: 42,
                id32: 7,
                row_tenant_id: "acme".to_string(),
                maybe_rank: None,
                score: 0.75,
                uuid_value: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                processed_at: "2024-01-02 03:04:05.123".to_string(),
                account_id: Some("account-1".to_string()),
                source_type: None,
                count_value: 99,
                status_code: 2,
                metric_a: 1.5,
                delta: -4,
                theme: "theme".to_string(),
                sub_theme: "sub-theme".to_string(),
                embedding: vec![1.0, 0.5],
            }
        );

        let tuple_value: (String, i64) = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Tuple<(
                diesel::sql_types::Text,
                diesel::sql_types::BigInt,
            )>,
        >("tuple('north', toInt64(7))"))
        .get_result(&mut conn)
        .await?;
        assert_eq!(tuple_value, ("north".to_string(), 7));

        let tuple_array: Vec<(String, i64)> = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Array<
                diesel_clickhouse::sql_types::Tuple<(
                    diesel::sql_types::Text,
                    diesel::sql_types::BigInt,
                )>,
            >,
        >(
            "[('north', toInt64(7)), ('south', toInt64(9))]",
        ))
        .get_result(&mut conn)
        .await?;
        assert_eq!(
            tuple_array,
            vec![("north".to_string(), 7), ("south".to_string(), 9)]
        );

        let decimal_value: String = diesel::select(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::Decimal64<2>,
        >("toDecimal64(123.45, 2)"))
        .get_result(&mut conn)
        .await?;
        assert_eq!(decimal_value, "123.45");

        #[cfg(feature = "bigdecimal")]
        {
            use std::str::FromStr;

            use bigdecimal::BigDecimal;

            let numeric_decimal: BigDecimal =
                diesel::select(diesel::dsl::sql::<diesel::sql_types::Numeric>(
                    "toDecimal128('1234567890.123456', 6)",
                ))
                .get_result(&mut conn)
                .await?;
            assert_eq!(numeric_decimal, BigDecimal::from_str("1234567890.123456")?);

            let decimal_family: (BigDecimal, BigDecimal, BigDecimal, BigDecimal) =
                diesel::select((
                    diesel::dsl::sql::<diesel_clickhouse::sql_types::Decimal32<2>>(
                        "toDecimal32('12.34', 2)",
                    ),
                    diesel::dsl::sql::<diesel_clickhouse::sql_types::Decimal64<4>>(
                        "toDecimal64('-56.7890', 4)",
                    ),
                    diesel::dsl::sql::<diesel_clickhouse::sql_types::Decimal128<8>>(
                        "toDecimal128('123456789.12345678', 8)",
                    ),
                    diesel::dsl::sql::<diesel_clickhouse::sql_types::Decimal256<12>>(
                        "toDecimal256('12345678901234567890.123456789012', 12)",
                    ),
                ))
                .get_result(&mut conn)
                .await?;
            assert_eq!(
                decimal_family,
                (
                    BigDecimal::from_str("12.34")?,
                    BigDecimal::from_str("-56.7890")?,
                    BigDecimal::from_str("123456789.12345678")?,
                    BigDecimal::from_str("12345678901234567890.123456789012")?,
                )
            );

            #[derive(Debug, PartialEq, QueryableByName)]
            struct BigDecimalBindRow {
                #[diesel(column_name = value)]
                #[diesel(sql_type = diesel_clickhouse::sql_types::Decimal64<2>)]
                value: BigDecimal,
            }

            let bound_decimal = diesel::sql_query("SELECT CAST(?, 'Decimal64(2)') AS value")
                .bind::<diesel_clickhouse::sql_types::Decimal64<2>, _>(BigDecimal::from_str(
                    "987.65",
                )?)
                .get_result::<BigDecimalBindRow>(&mut conn)
                .await?;
            assert_eq!(
                bound_decimal,
                BigDecimalBindRow {
                    value: BigDecimal::from_str("987.65")?,
                }
            );
        }

        let maybe_value: Option<String> = diesel::select(diesel::dsl::sql::<
            diesel::sql_types::Nullable<diesel::sql_types::Text>,
        >("NULL"))
        .get_result(&mut conn)
        .await?;
        assert_eq!(maybe_value, None);

        let nullable_scalars: (Option<i32>, Option<bool>, Option<String>) = diesel::select((
            diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Integer>>(
                "toNullable(toInt32(42))",
            ),
            diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Bool>>(
                "CAST(NULL, 'Nullable(Bool)')",
            ),
            diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Text>>(
                "toNullable('semi;colon?')",
            ),
        ))
        .get_result(&mut conn)
        .await?;
        assert_eq!(
            nullable_scalars,
            (Some(42), None, Some("semi;colon?".to_string()))
        );

        let nullable_composites = diesel::sql_query(
            "SELECT [toNullable('paid'), NULL, toNullable('mobile')] AS nullable_tags, map('present', toNullable(toInt32(7)), 'missing', CAST(NULL, 'Nullable(Int32)')) AS nullable_attrs, tuple('north', CAST(NULL, 'Nullable(Int64)')) AS nullable_tuple",
        )
        .get_result::<ConnectionNullableCompositeRow>(&mut conn).await?;
        assert_eq!(
            nullable_composites,
            ConnectionNullableCompositeRow {
                row_nullable_tags: vec![Some("paid".to_string()), None, Some("mobile".to_string()),],
                row_nullable_attrs: BTreeMap::from([
                    ("missing".to_string(), None),
                    ("present".to_string(), Some(7)),
                ]),
                row_nullable_tuple: ("north".to_string(), None),
            }
        );

        let semi_structured = diesel::sql_query(
            "SELECT CAST(toUInt64(42), 'Dynamic') AS dynamic_value, CAST(toUInt64(42), 'Variant(UInt64, String)') AS variant_value SETTINGS allow_experimental_dynamic_type = 1, allow_experimental_variant_type = 1",
        )
        .get_result::<ConnectionSemiStructuredRow>(&mut conn).await?;
        assert_eq!(
            semi_structured,
            ConnectionSemiStructuredRow {
                row_dynamic: "42".to_string(),
                row_variant: "42".to_string(),
            }
        );

        let literal_question_rows: Vec<String> = events
            .filter(tenant_id.eq("acme"))
            .select(diesel::dsl::sql::<diesel::sql_types::Text>("'literal ?'"))
            .order(id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(literal_question_rows, vec!["literal ?".to_string(); 3]);

        let server_parameter_text: String = diesel::select(
            diesel::dsl::sql::<diesel::sql_types::Text>("")
                .bind::<diesel::sql_types::Text, _>("quote ' and question ? stay data"),
        )
        .get_result(&mut conn)
        .await?;
        assert_eq!(server_parameter_text, "quote ' and question ? stay data");

        let server_parameter_null: Option<i32> = diesel::select(
            diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Integer>>("")
                .bind::<diesel::sql_types::Nullable<diesel::sql_types::Integer>, _>(None::<i32>),
        )
        .get_result(&mut conn)
        .await?;
        assert_eq!(server_parameter_null, None);

        diesel::sql_query("DROP TABLE IF EXISTS diesel_clickhouse_gold_documents")
            .execute(&mut conn)
            .await?;
        diesel::sql_query(
            "CREATE TABLE diesel_clickhouse_gold_documents (
                id UInt32,
                tenant_id String,
                text String,
                source_type String,
                processed_at DateTime64(3),
                embedding Array(Float32)
            ) ENGINE = Memory",
        )
        .execute(&mut conn)
        .await?;
        diesel::sql_query(
            r#"INSERT INTO diesel_clickhouse_gold_documents VALUES
                (1, 'acme', 'Rust ? async guide', 'blog', '2024-01-01 00:00:00.000', [1.0, 0.0]),
                (2, 'acme', 'ClickHouse diesel notes', '', '2024-01-01 00:05:00.000', [0.2, 0.8]),
                (3, 'beta', 'Rust analytics', 'doc', '2024-01-01 00:10:00.000', [0.4, 0.6]),
                (4, 'acme', 'rust diesel clickhouse', 'doc', '2024-01-01 00:15:00.000', [0.9, 0.1])"#,
        )
        .execute(&mut conn)
        .await?;

        let needle = "rust";
        let score_expr = diesel::dsl::sql::<diesel::sql_types::Float>(
            "toFloat32(if(positionCaseInsensitive(text, ",
        )
        .bind::<diesel::sql_types::Text, _>(needle)
        .sql(") > 0, 1, 0)) AS score");
        let match_filter =
            diesel::dsl::sql::<diesel::sql_types::Bool>("positionCaseInsensitive(text, ")
                .bind::<diesel::sql_types::Text, _>(needle)
                .sql(") > 0 AND text != '?' /* ? in comment is not a bind */");
        let search_rows: Vec<(u32, String, f32)> = gold_documents::table
            .filter(gold_documents::tenant_id.eq("acme"))
            .filter(match_filter)
            .select((gold_documents::id, gold_documents::text, score_expr))
            .order(diesel::dsl::sql::<diesel::sql_types::Float>("score").desc())
            .then_order_by(gold_documents::processed_at.desc())
            .then_order_by(gold_documents::id.asc())
            .limit(10_i64)
            .load(&mut conn)
            .await?;
        assert_eq!(
            search_rows,
            vec![
                (4, "rust diesel clickhouse".to_string(), 1.0),
                (1, "Rust ? async guide".to_string(), 1.0),
            ]
        );

        let optional_neither: Vec<i64> = events
            .filter(diesel_clickhouse::when(false, tenant_id.eq("ignored")))
            .filter(diesel_clickhouse::when(false, success.eq(false)))
            .select(id)
            .order(id.asc())
            .limit(2_i64)
            .load(&mut conn)
            .await?;
        assert_eq!(optional_neither, vec![1, 2]);

        let optional_tenant_only: Vec<i64> = events
            .filter(diesel_clickhouse::when(true, tenant_id.eq("beta")))
            .filter(diesel_clickhouse::when(false, success.eq(false)))
            .select(id)
            .order(id.asc())
            .limit(2_i64)
            .load(&mut conn)
            .await?;
        assert_eq!(optional_tenant_only, vec![4, 5]);

        let optional_success_only: Vec<i64> = events
            .filter(diesel_clickhouse::when(false, tenant_id.eq("ignored")))
            .filter(diesel_clickhouse::when(true, success.eq(false)))
            .select(id)
            .order(id.asc())
            .limit(2_i64)
            .load(&mut conn)
            .await?;
        assert_eq!(optional_success_only, vec![2, 6]);

        let optional_both: Vec<i64> = events
            .filter(diesel_clickhouse::when(true, tenant_id.eq("beta")))
            .filter(diesel_clickhouse::when(true, success.eq(false)))
            .select(id)
            .order(id.asc())
            .limit(2_i64)
            .load(&mut conn)
            .await?;
        assert_eq!(optional_both, vec![6]);

        type UInt64Array =
            diesel_clickhouse::sql_types::Array<diesel_clickhouse::sql_types::UInt64>;
        type TextArray = diesel_clickhouse::sql_types::Array<diesel::sql_types::Text>;
        type Float32Array = diesel_clickhouse::sql_types::Array<diesel::sql_types::Float>;

        let selected_by_u64_array: Vec<i64> = events
            .filter(
                diesel::dsl::sql::<diesel::sql_types::Bool>("has(")
                    .bind::<UInt64Array, _>(diesel_clickhouse::bind::<UInt64Array, _>(vec![
                        1_u64, 4_u64,
                    ]))
                    .sql(", toUInt64(id))"),
            )
            .select(id)
            .order(id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(selected_by_u64_array, vec![1, 4]);

        let selected_by_empty_u64_array: Vec<i64> = events
            .filter(
                diesel::dsl::sql::<diesel::sql_types::Bool>("has(")
                    .bind::<UInt64Array, _>(diesel_clickhouse::bind::<UInt64Array, _>(
                        Vec::<u64>::new(),
                    ))
                    .sql(", toUInt64(id))"),
            )
            .select(id)
            .order(id.asc())
            .load(&mut conn)
            .await?;
        assert!(selected_by_empty_u64_array.is_empty());

        let selected_by_parallel_string_arrays: Vec<i64> = events
            .filter(array_exists2(
                lambda2(
                    "allowed_tenant",
                    "allowed_status",
                    "allowed_tenant = tenant_id AND allowed_status = if(success, 'ok', 'fail')",
                ),
                diesel_clickhouse::bind::<TextArray, _>(vec![
                    "acme".to_string(),
                    "beta".to_string(),
                ]),
                diesel_clickhouse::bind::<TextArray, _>(vec!["ok".to_string(), "fail".to_string()]),
            ))
            .select(id)
            .order(id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(selected_by_parallel_string_arrays, vec![1, 3, 6]);

        let helper_rows: Vec<(u32, String, Option<String>, i64)> = gold_documents::table
            .filter(gold_documents::tenant_id.eq("acme"))
            .filter(
                position_case_insensitive(gold_documents::text, "RUST")
                    .gt(diesel_clickhouse::bind(0_u64)),
            )
            .select((
                gold_documents::id,
                left_utf8(gold_documents::text, 4_i64),
                null_if(gold_documents::source_type, ""),
                length_utf8(gold_documents::text),
            ))
            .order(gold_documents::id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(
            helper_rows,
            vec![
                (1, "Rust".to_string(), Some("blog".to_string()), 18),
                (4, "rust".to_string(), Some("doc".to_string()), 22),
            ]
        );

        let vector_scores: Vec<(u32, f32)> = gold_documents::table
            .filter(gold_documents::tenant_id.eq("acme"))
            .select((
                gold_documents::id,
                expr_as(
                    vector_dot_product_f32(
                        gold_documents::embedding,
                        diesel_clickhouse::bind::<Float32Array, _>(vec![1.0_f32, 0.0_f32]),
                    ),
                    "score",
                ),
            ))
            .order(alias_ref::<diesel::sql_types::Float>("score").desc())
            .then_order_by(gold_documents::id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(vector_scores, vec![(1, 1.0), (4, 0.9), (2, 0.2)]);

        let reusable_query_vector = named_param::<Float32Array, _>("q", vec![1.0_f32, 0.0_f32]);
        let named_vector_scores: Vec<(u32, f32)> = gold_documents::table
            .filter(gold_documents::tenant_id.eq("acme"))
            .filter(
                vector_dot_product_f32(gold_documents::embedding, reusable_query_vector.clone())
                    .gt(0.5_f32),
            )
            .select((
                gold_documents::id,
                expr_as(
                    vector_dot_product_f32(gold_documents::embedding, reusable_query_vector),
                    "score",
                ),
            ))
            .order(alias_ref::<diesel::sql_types::Float>("score").desc())
            .then_order_by(gold_documents::id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(named_vector_scores, vec![(1, 1.0), (4, 0.9)]);

        let conflicting_named_parameter = diesel::select((
            named_param::<diesel_clickhouse::sql_types::UInt64, _>("same", 1_u64),
            named_param::<diesel_clickhouse::sql_types::UInt64, _>("same", 2_u64),
        ))
        .load::<(u64, u64)>(&mut conn)
        .await
        .expect_err("conflicting named parameter values should fail before execution");
        assert!(
            conflicting_named_parameter
                .to_string()
                .contains("conflicting values for ClickHouse named parameter"),
            "unexpected error: {conflicting_named_parameter}"
        );

        let row_bridge_scores: Vec<ClickHouseDocumentScore> = conn
            .load_clickhouse_rows(
                gold_documents::table
                    .filter(gold_documents::tenant_id.eq("acme"))
                    .select((
                        gold_documents::id,
                        gold_documents::text,
                        expr_as(
                            vector_dot_product_f32(
                                gold_documents::embedding,
                                diesel_clickhouse::bind::<Float32Array, _>(vec![1.0_f32, 0.0_f32]),
                            ),
                            "score",
                        ),
                    ))
                    .order(alias_ref::<diesel::sql_types::Float>("score").desc())
                    .then_order_by(gold_documents::id.asc()),
            )
            .await?;
        assert_eq!(
            row_bridge_scores,
            vec![
                ClickHouseDocumentScore {
                    id: 1,
                    text: "Rust ? async guide".to_string(),
                    score: 1.0,
                },
                ClickHouseDocumentScore {
                    id: 4,
                    text: "rust diesel clickhouse".to_string(),
                    score: 0.9,
                },
                ClickHouseDocumentScore {
                    id: 2,
                    text: "ClickHouse diesel notes".to_string(),
                    score: 0.2,
                },
            ]
        );
        diesel::sql_query("DROP TABLE diesel_clickhouse_gold_documents")
            .execute(&mut conn)
            .await?;

        diesel::sql_query("DROP TABLE IF EXISTS diesel_clickhouse_connection_spike")
            .execute(&mut conn)
            .await?;
        diesel::sql_query(
            "CREATE TABLE diesel_clickhouse_connection_spike (id Int64, tenant_id String) ENGINE = Memory",
        )
        .execute(&mut conn).await?;
        diesel::sql_query(
            "INSERT INTO diesel_clickhouse_connection_spike VALUES (1, 'acme'), (2, 'beta')",
        )
        .execute(&mut conn)
        .await?;
        let sql_rows = diesel::sql_query(
            "SELECT id, tenant_id FROM diesel_clickhouse_connection_spike ORDER BY id",
        )
        .load::<ConnectionRow>(&mut conn)
        .await?;
        assert_eq!(
            sql_rows,
            vec![
                ConnectionRow {
                    row_id: 1,
                    row_tenant_id: "acme".to_string(),
                },
                ConnectionRow {
                    row_id: 2,
                    row_tenant_id: "beta".to_string(),
                },
            ]
        );
        diesel::sql_query("DROP TABLE diesel_clickhouse_connection_spike")
            .execute(&mut conn)
            .await?;

        // Idiomatic Diesel single-row inserts through the connection: both the
        // explicit-tuple form and a `#[derive(Insertable)]` struct.
        diesel::sql_query("DROP TABLE IF EXISTS diesel_clickhouse_connection_inserts")
            .execute(&mut conn)
            .await?;
        diesel::sql_query(
            "CREATE TABLE diesel_clickhouse_connection_inserts (id Int64, tenant_id String) \
             ENGINE = Memory",
        )
        .execute(&mut conn)
        .await?;

        // `execute` now surfaces the written-row count ClickHouse reports in its
        // `X-ClickHouse-Summary` trailer, so a single-row insert returns 1.
        let tuple_insert_count = diesel::insert_into(connection_inserts::table)
            .values((
                connection_inserts::id.eq(1_i64),
                connection_inserts::tenant_id.eq("acme"),
            ))
            .execute(&mut conn)
            .await?;
        assert_eq!(tuple_insert_count, 1);
        let struct_insert_count = diesel::insert_into(connection_inserts::table)
            .values(&NewConnectionInsert {
                id: 2,
                tenant_id: "beta",
            })
            .execute(&mut conn)
            .await?;
        assert_eq!(struct_insert_count, 1);

        let inserted_rows: Vec<(i64, String)> = connection_inserts::table
            .select((connection_inserts::id, connection_inserts::tenant_id))
            .order(connection_inserts::id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(
            inserted_rows,
            vec![(1, "acme".to_string()), (2, "beta".to_string())]
        );
        diesel::sql_query("DROP TABLE diesel_clickhouse_connection_inserts")
            .execute(&mut conn)
            .await?;

        // Multi-row ingestion via `insert_batch`: one columnar RowBinary request
        // for the whole batch, returning the number of rows sent.
        diesel::sql_query("DROP TABLE IF EXISTS diesel_clickhouse_connection_batch")
            .execute(&mut conn)
            .await?;
        diesel::sql_query(
            "CREATE TABLE diesel_clickhouse_connection_batch (id Int64, tenant_id String) \
             ENGINE = Memory",
        )
        .execute(&mut conn)
        .await?;

        let batch = vec![
            BatchRow {
                id: 10,
                tenant_id: "acme".to_string(),
            },
            BatchRow {
                id: 11,
                tenant_id: "beta".to_string(),
            },
            BatchRow {
                id: 12,
                tenant_id: "gamma".to_string(),
            },
        ];
        let batch_count = conn
            .insert_batch_with_options(
                "diesel_clickhouse_connection_batch",
                batch,
                InsertBatchOptions::new()
                    .timeouts(Some(Duration::from_secs(5)), Some(Duration::from_secs(10)))
                    .setting("query_id", "diesel_clickhouse_connection_batch_options"),
            )
            .await?;
        assert_eq!(batch_count, 3);

        let batch_rows: Vec<(i64, String)> = connection_batch::table
            .select((connection_batch::id, connection_batch::tenant_id))
            .order(connection_batch::id.asc())
            .load(&mut conn)
            .await?;
        assert_eq!(
            batch_rows,
            vec![
                (10, "acme".to_string()),
                (11, "beta".to_string()),
                (12, "gamma".to_string()),
            ]
        );
        diesel::sql_query("DROP TABLE diesel_clickhouse_connection_batch")
            .execute(&mut conn)
            .await?;

        // Type-safe column selection from a ClickHouse join via `join_column`,
        // loaded through the Diesel connection into a typed `(i64, String)` tuple
        // (no hand-written `sql::<...>("...")` projection).
        let typed_join_rows: Vec<(i64, String)> = events
            .clickhouse_join(tenants::table)
            .all()
            .inner()
            .on(tenant_id.eq(tenants::tenant_id))
            .select((join_column(id), join_column(tenants::plan)))
            .order(join_column(id).asc())
            .load(&mut conn)
            .await?;
        assert_eq!(
            typed_join_rows,
            vec![
                (1, "enterprise".to_string()),
                (2, "enterprise".to_string()),
                (3, "enterprise".to_string()),
                (4, "starter".to_string()),
                (5, "starter".to_string()),
                (6, "starter".to_string()),
            ]
        );

        // Typed predicates over a custom join, executed end to end: a typed
        // `.on(...)`, typed `.filter(...)` against columns from *both* sides of
        // the join, and a native `count()` loaded into `u64` (ClickHouse's
        // `count()` is `UInt64`). No `sql::<Bool>("...")` predicate anywhere.
        let enterprise_successes: Vec<(String, u64)> = events
            .clickhouse_join(tenants::table)
            .all()
            .inner()
            .on(tenant_id.eq(tenants::tenant_id))
            .select((source_column(tenants::plan), expr_as(count(), "n")))
            .filter(tenants::plan.eq("enterprise"))
            .filter(success.eq(true))
            .group_by(join_column(tenants::plan))
            .load(&mut conn)
            .await?;
        // Enterprise = tenant `acme` (events 1,2,3); successes among them: 1 and 3.
        assert_eq!(enterprise_successes, vec![("enterprise".to_string(), 2)]);

        conn.batch_execute(
            r#"
            DROP TABLE IF EXISTS diesel_clickhouse_connection_batch;
            CREATE TABLE diesel_clickhouse_connection_batch (id Int64, tenant_id String) ENGINE = Memory;
            INSERT INTO diesel_clickhouse_connection_batch VALUES (1, 'literal;with;semicolons');
            INSERT INTO diesel_clickhouse_connection_batch VALUES (2, 'escaped \' quote; still literal');
            -- trailing comment with a semicolon ; should not become a statement
            "#,
        )
        .await?;
        let batch_rows = diesel::sql_query(
            "SELECT id, tenant_id FROM diesel_clickhouse_connection_batch ORDER BY id",
        )
        .load::<ConnectionRow>(&mut conn)
        .await?;
        assert_eq!(
            batch_rows,
            vec![
                ConnectionRow {
                    row_id: 1,
                    row_tenant_id: "literal;with;semicolons".to_string(),
                },
                ConnectionRow {
                    row_id: 2,
                    row_tenant_id: "escaped ' quote; still literal".to_string(),
                },
            ]
        );
        conn.batch_execute("DROP TABLE diesel_clickhouse_connection_batch; /* done ; */")
            .await?;
    }

    let ddl = create_table("diesel_clickhouse_ddl_events")
        .if_not_exists()
        .column("id", DataType::UInt64)
        .column("tenant_id", DataType::low_cardinality(DataType::String))
        .column("created_at", DataType::DateTime)
        .column_def(Column::new("created_date", DataType::Date).alias_expr("toDate(created_at)"))
        .column("success", DataType::Bool)
        .column("latency_ms", DataType::Float64)
        .engine(
            replacing_merge_tree()
                .partition_by(["toDate(created_at)"])
                .primary_key(["tenant_id", "id"])
                .order_by(["tenant_id", "id"])
                .sample_by("id")
                .ttl("created_at + INTERVAL 7 DAY")
                .setting("index_granularity", 8192_i64),
        );
    fixture
        .client
        .query("DROP TABLE IF EXISTS diesel_clickhouse_ddl_events")
        .execute()
        .await?;
    fixture.client.query(&to_sql(&ddl)?).execute().await?;
    fixture
        .client
        .query("INSERT INTO diesel_clickhouse_ddl_events (id, tenant_id, created_at, success, latency_ms) VALUES (1, 'acme', '2024-01-01 00:00:00', true, 12.5)")
        .execute()
        .await?;
    let ddl_row: (String, u64, String) = fixture
        .client
        .query("SELECT tenant_id, count(), toString(any(created_date)) FROM diesel_clickhouse_ddl_events GROUP BY tenant_id")
        .fetch_one()
        .await?;
    assert_eq!(ddl_row, ("acme".to_string(), 1, "2024-01-01".to_string()));

    let type_showcase_ddl = create_table("diesel_clickhouse_type_showcase")
        .column("big_signed", DataType::Int128)
        .column("huge_signed", DataType::Int256)
        .column("big_unsigned", DataType::UInt128)
        .column("huge_unsigned", DataType::UInt256)
        .column("amount32", DataType::decimal32(2))
        .column("amount64", DataType::decimal64(4))
        .column("amount128", DataType::decimal128(8))
        .column("amount256", DataType::decimal256(12))
        .column("amount", DataType::decimal(18, 6))
        .column("status", DataType::enum8([("draft", 1), ("published", 2)]))
        .column("kind", DataType::enum16([("organic", 100), ("paid", 200)]))
        .column("location", DataType::Point)
        .column("boundary", DataType::Ring)
        .column("flex", DataType::dynamic_with_max_types(4))
        .column(
            "variant_value",
            DataType::variant([
                DataType::UInt64,
                DataType::String,
                DataType::array(DataType::UInt64),
            ]),
        )
        .column(
            "dimensions",
            DataType::tuple([DataType::String, DataType::UInt64, DataType::Float64]),
        )
        .column(
            "attributes",
            DataType::nested([
                NestedField::new("key", DataType::String),
                NestedField::new("value", DataType::String),
            ]),
        )
        .engine(TableEngine::memory());
    fixture
        .client
        .query(&to_sql(&type_showcase_ddl)?)
        .with_setting("allow_experimental_dynamic_type", "1")
        .with_setting("allow_experimental_variant_type", "1")
        .execute()
        .await?;
    let type_showcase_exists: u64 = fixture
        .client
        .query("SELECT count() FROM system.tables WHERE database = currentDatabase() AND name = 'diesel_clickhouse_type_showcase'")
        .fetch_one()
        .await?;
    assert_eq!(type_showcase_exists, 1);

    let projection_rollups_ddl = create_table("diesel_clickhouse_projection_rollups")
        .column("tenant_id", DataType::String)
        .column("bucket", DataType::Date)
        .column("hits", DataType::UInt64)
        .projection(projection(
            "by_tenant",
            "SELECT tenant_id, sum(hits) GROUP BY tenant_id",
        ))
        .engine(merge_tree().order_by(["tenant_id", "bucket"]));
    fixture
        .client
        .query(&to_sql(&projection_rollups_ddl)?)
        .execute()
        .await?;
    let projection_exists: u64 = fixture
        .client
        .query("SELECT count() FROM system.tables WHERE database = currentDatabase() AND name = 'diesel_clickhouse_projection_rollups' AND create_table_query LIKE '%PROJECTION%by_tenant%'")
        .fetch_one()
        .await?;
    assert_eq!(projection_exists, 1);

    let summing_rollups_ddl = create_table("diesel_clickhouse_summing_rollups")
        .column("tenant_id", DataType::String)
        .column("bucket", DataType::Date)
        .column("hits", DataType::UInt64)
        .engine(summing_merge_tree().order_by(["tenant_id", "bucket"]));
    fixture
        .client
        .query(&to_sql(&summing_rollups_ddl)?)
        .execute()
        .await?;
    let summing_exists: u64 = fixture
        .client
        .query("SELECT count() FROM system.tables WHERE database = currentDatabase() AND name = 'diesel_clickhouse_summing_rollups'")
        .fetch_one()
        .await?;
    assert_eq!(summing_exists, 1);

    let mutations_ddl = create_table("diesel_clickhouse_mutations")
        .column("id", DataType::UInt64)
        .column("day", DataType::Date)
        .column("value", DataType::UInt64)
        .column("label", DataType::String)
        .engine(
            merge_tree()
                .partition_by(["day"])
                .order_by(["id"])
                .setting("index_granularity", 128_i64),
        );
    fixture
        .client
        .query(&to_sql(&mutations_ddl)?)
        .execute()
        .await?;
    fixture
        .client
        .query(
            "INSERT INTO diesel_clickhouse_mutations VALUES \
            (1, '2024-01-01', 1, 'old'), \
            (2, '2024-01-01', 2, 'delete-me'), \
            (3, '2024-01-02', 3, 'keep')",
        )
        .execute()
        .await?;
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_mutations")
                .update(
                    [
                        mutation_assignment("value", "value + 10"),
                        mutation_assignment("label", "'updated'"),
                    ],
                    "id = 1",
                )
                .setting("mutations_sync", 2_i64),
        )?)
        .execute()
        .await?;
    let updated_row: (u64, String) = fixture
        .client
        .query("SELECT value, label FROM diesel_clickhouse_mutations WHERE id = 1")
        .fetch_one()
        .await?;
    assert_eq!(updated_row, (11, "updated".to_string()));
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_mutations")
                .delete_in_partition(partition_expr("'2024-01-01'"), "id = 2")
                .setting("mutations_sync", 2_i64),
        )?)
        .execute()
        .await?;
    let mutation_count: u64 = fixture
        .client
        .query("SELECT count() FROM diesel_clickhouse_mutations")
        .fetch_one()
        .await?;
    assert_eq!(mutation_count, 2);

    let partitions_ddl = create_table("diesel_clickhouse_partitions")
        .column("id", DataType::UInt64)
        .column("day", DataType::Date)
        .column("value", DataType::String)
        .engine(merge_tree().partition_by(["day"]).order_by(["id"]));
    fixture
        .client
        .query(&to_sql(&partitions_ddl)?)
        .execute()
        .await?;
    fixture
        .client
        .query(
            "INSERT INTO diesel_clickhouse_partitions VALUES \
            (1, '2024-01-01', 'a'), \
            (2, '2024-01-01', 'b'), \
            (3, '2024-01-02', 'c')",
        )
        .execute()
        .await?;
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_partitions")
                .detach_partition(partition_expr("'2024-01-01'")),
        )?)
        .execute()
        .await?;
    let detached_count: u64 = fixture
        .client
        .query("SELECT count() FROM diesel_clickhouse_partitions")
        .fetch_one()
        .await?;
    assert_eq!(detached_count, 1);
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_partitions")
                .attach_partition(partition_expr("'2024-01-01'")),
        )?)
        .execute()
        .await?;
    let attached_count: u64 = fixture
        .client
        .query("SELECT count() FROM diesel_clickhouse_partitions")
        .fetch_one()
        .await?;
    assert_eq!(attached_count, 3);
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_partitions")
                .drop_partition(partition_expr("'2024-01-02'")),
        )?)
        .execute()
        .await?;
    let partition_count_after_drop: u64 = fixture
        .client
        .query("SELECT count() FROM diesel_clickhouse_partitions")
        .fetch_one()
        .await?;
    assert_eq!(partition_count_after_drop, 2);

    let aggregate_states_ddl = create_table("diesel_clickhouse_aggregate_states")
        .column("state_key", DataType::UInt64)
        .column(
            "latency_sum",
            DataType::aggregate_function("sum", [DataType::Float64]),
        )
        .column("event_count", DataType::aggregate_function("count", []))
        .column(
            "exact_ids",
            DataType::aggregate_function("uniqExact", [DataType::UInt64]),
        )
        .engine(aggregating_merge_tree().order_by(["state_key"]));
    fixture
        .client
        .query(&to_sql(&aggregate_states_ddl)?)
        .execute()
        .await?;
    fixture
        .client
        .query(
            "INSERT INTO diesel_clickhouse_aggregate_states \
            SELECT 1, sumState(latency_ms), countState(), uniqExactState(id) \
            FROM diesel_clickhouse_events",
        )
        .execute()
        .await?;
    let aggregate_merge_sql = to_sql(&aggregate_states::table.select((
        sum_merge(aggregate_states::latency_sum),
        count_merge(aggregate_states::event_count),
        uniq_exact_merge(aggregate_states::exact_ids),
    )))?;
    let aggregate_merge_row: (f64, u64, u64) = fixture
        .client
        .query(&aggregate_merge_sql)
        .fetch_one()
        .await?;
    assert_eq!(aggregate_merge_row, (210.0, 6, 6));

    let finalized_sum_sql = to_sql(&events.select(finalize_aggregation(sum_state(latency_ms))))?;
    let finalized_sum: f64 = fixture.client.query(&finalized_sum_sql).fetch_one().await?;
    assert_eq!(finalized_sum, 210.0);

    let mv_source_ddl = create_table("diesel_clickhouse_mv_source")
        .column("id", DataType::Int64)
        .column("tenant_id", DataType::String)
        .column("success", DataType::Bool)
        .engine(TableEngine::custom("MergeTree ORDER BY id"));
    let mv_target_ddl = create_table("diesel_clickhouse_mv_target")
        .column("tenant_id", DataType::String)
        .column("success_count", DataType::UInt64)
        .engine(TableEngine::memory());
    fixture
        .client
        .query(&to_sql(&mv_source_ddl)?)
        .execute()
        .await?;
    fixture
        .client
        .query(&to_sql(&mv_target_ddl)?)
        .execute()
        .await?;
    let mv_ddl =
        create_materialized_view("diesel_clickhouse_mv")
            .to("diesel_clickhouse_mv_target")
            .as_select(mv_source::table.group_by(mv_source::tenant_id).select(
                diesel::dsl::sql::<(
                    diesel::sql_types::Text,
                    diesel_clickhouse::sql_types::UInt64,
                )>("tenant_id, countIf(success) AS success_count"),
            ));
    fixture.client.query(&to_sql(&mv_ddl)?).execute().await?;
    fixture
        .client
        .query(
            "INSERT INTO diesel_clickhouse_mv_source VALUES \
            (1, 'acme', true), \
            (2, 'acme', false), \
            (3, 'acme', true), \
            (4, 'beta', true)",
        )
        .execute()
        .await?;
    let mv_rows: Vec<(String, u64)> = fixture
        .client
        .query(
            "SELECT tenant_id, success_count \
            FROM diesel_clickhouse_mv_target \
            ORDER BY tenant_id",
        )
        .fetch_all()
        .await?;
    assert_eq!(
        mv_rows,
        vec![("acme".to_string(), 2), ("beta".to_string(), 1)]
    );

    let vector_table_ddl = create_table("diesel_clickhouse_image_vectors")
        .column("id", DataType::Int64)
        .column("caption", DataType::String)
        .column_def(Column::new("embedding", DataType::array(DataType::Float32)).codec("NONE"))
        .engine(TableEngine::custom("MergeTree ORDER BY id"));
    fixture
        .client
        .query(&to_sql(&vector_table_ddl)?)
        .execute()
        .await?;
    fixture
        .client
        .query(
            "INSERT INTO diesel_clickhouse_image_vectors VALUES \
            (1, 'dog', [1.0, 0.0]), \
            (2, 'puppy', [1.1, 0.0]), \
            (3, 'cat', [0.0, 1.0])",
        )
        .execute()
        .await?;
    let reference_vector = vector_f32([1.0, 0.0]);
    let vector_search_sql = to_sql(
        &image_vectors::table
            .select((
                image_vectors::id,
                image_vectors::caption,
                to_float64(l2_distance(
                    image_vectors::embedding,
                    reference_vector.clone(),
                )),
            ))
            .order(l2_distance(image_vectors::embedding, reference_vector).asc())
            .limit(2),
    )?;
    let vector_rows: Vec<(i64, String, f64)> = fixture
        .client
        .query(&vector_search_sql)
        .bind(2_i64)
        .fetch_all()
        .await?;
    assert_eq!(vector_rows[0].0, 1);
    assert_eq!(vector_rows[0].1, "dog");
    assert!(vector_rows[0].2.abs() < f64::EPSILON);
    assert_eq!(vector_rows[1].0, 2);
    assert_eq!(vector_rows[1].1, "puppy");
    assert!((vector_rows[1].2 - 0.1).abs() < 0.000_001);

    let add_column_sql = to_sql(&alter_table("diesel_clickhouse_events").add_column_after(
        Column::new("scratch", DataType::UInt64).default_expr("7"),
        "id",
    ))?;
    fixture.client.query(&add_column_sql).execute().await?;
    let scratch_sum: u64 = fixture
        .client
        .query("SELECT sum(scratch) FROM diesel_clickhouse_events")
        .fetch_one()
        .await?;
    assert_eq!(scratch_sum, 42);
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_events").rename_column("scratch", "scratch2"),
        )?)
        .execute()
        .await?;
    let scratch_sum_renamed: u64 = fixture
        .client
        .query("SELECT sum(scratch2) FROM diesel_clickhouse_events")
        .fetch_one()
        .await?;
    assert_eq!(scratch_sum_renamed, 42);
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_events").drop_column("scratch2"),
        )?)
        .execute()
        .await?;

    let index = TableIndex::custom("events_id_minmax", "id", "minmax").granularity(1);
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_events").add_index(index),
        )?)
        .execute()
        .await?;
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_events")
                .materialize_index("events_id_minmax")
                .setting("mutations_sync", 2_i64),
        )?)
        .execute()
        .await?;
    fixture
        .client
        .query(&to_sql(
            &alter_table("diesel_clickhouse_events").drop_index("events_id_minmax"),
        )?)
        .execute()
        .await?;

    let join_sql = to_sql(
        &events
            .clickhouse_join(tenants::table)
            .global()
            .any()
            .inner()
            .using(["tenant_id"])
            .select(diesel::dsl::sql::<(
                diesel::sql_types::BigInt,
                diesel::sql_types::Text,
            )>(
                "`diesel_clickhouse_events`.`id`, `diesel_clickhouse_tenants`.`plan`",
            ))
            .order(diesel::dsl::sql::<diesel::sql_types::BigInt>(
                "`diesel_clickhouse_events`.`id`",
            )),
    )?;
    let join_rows: Vec<(u64, String)> = fixture.client.query(&join_sql).fetch_all().await?;
    assert_eq!(
        join_rows,
        vec![(1, "enterprise".to_string()), (4, "starter".to_string())]
    );

    let semi_count_sql = to_sql(
        &events
            .clickhouse_join(tenants::table)
            .left()
            .semi()
            .using(["tenant_id"])
            .select(diesel::dsl::sql::<diesel::sql_types::BigInt>(
                "uniqExact(`diesel_clickhouse_events`.`tenant_id`)",
            )),
    )?;
    let semi_count: u64 = fixture.client.query(&semi_count_sql).fetch_one().await?;
    assert_eq!(semi_count, 2);

    let anti_sql = to_sql(
        &tenants::table
            .clickhouse_join(events)
            .left()
            .anti()
            .using(["tenant_id"])
            .select(diesel::dsl::sql::<diesel::sql_types::Text>(
                "`diesel_clickhouse_tenants`.`tenant_id`",
            )),
    )?;
    let anti_tenant: String = fixture.client.query(&anti_sql).fetch_one().await?;
    assert_eq!(anti_tenant, "gamma");

    let asof_sql = to_sql(
        &events
            .clickhouse_join(tenant_rates::table)
            .asof()
            .left()
            .on(diesel::dsl::sql::<diesel::sql_types::Bool>(
                "`diesel_clickhouse_events`.`tenant_id` = `diesel_clickhouse_tenant_rates`.`tenant_id` AND `diesel_clickhouse_events`.`created_at` >= `diesel_clickhouse_tenant_rates`.`effective_at`",
            ))
            .select(diesel::dsl::sql::<(
                diesel::sql_types::BigInt,
                diesel::sql_types::Double,
            )>(
                "`diesel_clickhouse_events`.`id`, `diesel_clickhouse_tenant_rates`.`rate`",
            ))
            .order(diesel::dsl::sql::<diesel::sql_types::BigInt>(
                "`diesel_clickhouse_events`.`id`",
            )),
    )?;
    let asof_rows: Vec<(u64, f64)> = fixture.client.query(&asof_sql).fetch_all().await?;
    assert_eq!(
        asof_rows,
        vec![(1, 1.0), (2, 1.0), (3, 2.0), (4, 3.0), (5, 3.0), (6, 3.0)]
    );

    let with_sql = to_sql(
        &diesel::select(diesel::dsl::sql::<diesel::sql_types::BigInt>("next_answer"))
            .with_alias(
                diesel::dsl::sql::<diesel::sql_types::BigInt>("toInt64(41)"),
                "answer",
            )
            .and_with_alias(
                diesel::dsl::sql::<diesel::sql_types::BigInt>("answer + toInt64(1)"),
                "next_answer",
            ),
    )?;
    let answer: i64 = fixture.client.query(&with_sql).fetch_one().await?;
    assert_eq!(answer, 42);

    let cte_sql = to_sql(
        &diesel::select(diesel::dsl::sql::<diesel::sql_types::BigInt>(
            "x FROM answer_cte",
        ))
        .with_cte(
            "answer_cte",
            diesel::select(diesel::dsl::sql::<diesel::sql_types::BigInt>(
                "toInt64(42) AS x",
            )),
        ),
    )?;
    let cte_answer: i64 = fixture.client.query(&cte_sql).fetch_one().await?;
    assert_eq!(cte_answer, 42);

    // Aggregates/functions + Diesel bind placeholders: render via this crate,
    // then bind the value with the official ClickHouse client.
    let analytics_query = events
        .filter(tenant_id.eq("acme"))
        .group_by(tenant_id)
        .select((tenant_id, count_if(success), quantile(0.5, latency_ms)))
        .order(tenant_id.asc());
    let analytics_sql = to_sql(&analytics_query)?;
    let analytics: (String, u64, f64) = fixture
        .client
        .query(&analytics_sql)
        .bind("acme")
        .fetch_one()
        .await?;
    assert_eq!(analytics.0, "acme");
    assert_eq!(analytics.1, 2, "countIf(success) should count true rows");
    assert!(
        (10.0..=30.0).contains(&analytics.2),
        "median-like quantile should be in the acme latency range, got {}",
        analytics.2
    );

    let aggregate_sql = to_sql(&events.select((
        min_if(latency_ms, success),
        max_if(latency_ms, success),
        uniq_exact_if(id, success),
        quantile_exact(0.5, latency_ms),
    )))?;
    let aggregate_row: (f64, f64, u64, f64) =
        fixture.client.query(&aggregate_sql).fetch_one().await?;
    assert_eq!(aggregate_row.0, 10.0);
    assert_eq!(aggregate_row.1, 50.0);
    assert_eq!(aggregate_row.2, 4);
    assert!((10.0..=60.0).contains(&aggregate_row.3));

    let stats_sql = to_sql(&events.select((
        stddev_pop(latency_ms),
        stddev_samp(latency_ms),
        stddev_pop_stable(latency_ms),
        var_pop(latency_ms),
        var_pop_stable(latency_ms),
        to_string(analysis_of_variance(latency_ms, success)),
        to_string(mann_whitney_u_test(latency_ms, success)),
        to_string(approx_top_sum(2, tenant_id, id)),
    )))?;
    let stats_row: (f64, f64, f64, f64, f64, String, String, String) =
        fixture.client.query(&stats_sql).fetch_one().await?;
    assert!(stats_row.0 > 0.0);
    assert!(stats_row.1 > 0.0);
    assert!(stats_row.2 > 0.0);
    assert!(stats_row.3 > 0.0);
    assert!(stats_row.4 > 0.0);
    assert!(stats_row.5.starts_with('('));
    assert!(stats_row.6.starts_with('('));
    assert!(stats_row.7.starts_with('['));

    let generic_aggregate_sql = to_sql(
        &events.select((
            aggregate::<diesel::sql_types::Double>("sum")
                .arg(latency_ms)
                .if_(success),
            aggregate::<diesel::sql_types::BigInt>("count")
                .no_args()
                .if_(success),
            aggregate::<diesel::sql_types::Double>("avg")
                .arg(latency_ms)
                .or_default()
                .if_(diesel::dsl::sql::<diesel::sql_types::Bool>(
                    "latency_ms > 1000",
                )),
        )),
    )?;
    let generic_aggregate_row: (f64, u64, f64) = fixture
        .client
        .query(&generic_aggregate_sql)
        .fetch_one()
        .await?;
    assert_eq!(generic_aggregate_row, (130.0, 4, 0.0));

    let quantiles_sql = to_sql(&events.select(quantiles([0.25, 0.5, 0.75], latency_ms)))?;
    let latency_quantiles: Vec<f64> = fixture.client.query(&quantiles_sql).fetch_one().await?;
    assert_eq!(latency_quantiles.len(), 3);

    let timing_quantile_sql = to_sql(&events.select(to_float64(quantile_timing(0.5, latency_ms))))?;
    let timing_quantile: f64 = fixture
        .client
        .query(&timing_quantile_sql)
        .fetch_one()
        .await?;
    assert!((10.0..=60.0).contains(&timing_quantile));

    let deterministic_quantile_sql =
        to_sql(&events.select(quantile_deterministic(0.5, latency_ms, id)))?;
    let deterministic_quantile: f64 = fixture
        .client
        .query(&deterministic_quantile_sql)
        .fetch_one()
        .await?;
    assert!((10.0..=60.0).contains(&deterministic_quantile));

    let quantiles_timing_len_sql =
        to_sql(&events.select(length(quantiles_timing([0.5, 0.95], latency_ms))))?;
    let timing_quantile_count: u64 = fixture
        .client
        .query(&quantiles_timing_len_sql)
        .fetch_one()
        .await?;
    assert_eq!(timing_quantile_count, 2);

    let histogram_len_sql = to_sql(&events.select(length(histogram(3, latency_ms))))?;
    let histogram_bucket_count: u64 = fixture.client.query(&histogram_len_sql).fetch_one().await?;
    assert_eq!(histogram_bucket_count, 3);

    let statistical_sql = to_sql(&events.select((
        corr(latency_ms, to_float64(id)),
        covar_pop(latency_ms, to_float64(id)),
        covar_samp(latency_ms, to_float64(id)),
        covar_pop_stable(latency_ms, to_float64(id)),
        covar_samp_stable(latency_ms, to_float64(id)),
    )))?;
    let statistical_row: (f64, f64, f64, f64, f64) =
        fixture.client.query(&statistical_sql).fetch_one().await?;
    assert!((statistical_row.0 - 1.0).abs() < 1e-12);
    assert!((statistical_row.1 - 29.166666666666668).abs() < 1e-9);
    assert!((statistical_row.2 - 35.0).abs() < 1e-9);
    assert!((statistical_row.3 - 29.166666666666668).abs() < 1e-9);
    assert!((statistical_row.4 - 35.0).abs() < 1e-9);

    let top_success_sql = to_sql(&events.select(top_k(1, success)))?;
    let top_successes: Vec<bool> = fixture.client.query(&top_success_sql).fetch_one().await?;
    assert_eq!(top_successes, vec![true]);

    let rollup_sql = to_sql(
        &events
            .group_by(rollup(tenant_id))
            .select((
                diesel::dsl::sql::<diesel::sql_types::Text>("tenant_id"),
                diesel::dsl::sql::<diesel::sql_types::BigInt>("count()"),
            ))
            .order(diesel::dsl::sql::<diesel::sql_types::Text>("tenant_id").asc()),
    )?;
    let rollup_rows: Vec<(String, u64)> = fixture.client.query(&rollup_sql).fetch_all().await?;
    assert_eq!(
        rollup_rows,
        vec![
            (String::new(), 6),
            ("acme".to_string(), 3),
            ("beta".to_string(), 3),
        ]
    );

    let group_by_all_sql = to_sql(
        &events
            .group_by(group_by_all())
            .select(diesel::dsl::sql::<(
                diesel::sql_types::Text,
                diesel::sql_types::BigInt,
            )>("tenant_id, count()"))
            .order(diesel::dsl::sql::<diesel::sql_types::Text>("tenant_id").asc()),
    )?;
    let group_by_all_rows: Vec<(String, u64)> =
        fixture.client.query(&group_by_all_sql).fetch_all().await?;
    assert_eq!(
        group_by_all_rows,
        vec![("acme".to_string(), 3), ("beta".to_string(), 3)]
    );

    let grouping_sets_sql = to_sql(
        &events
            .group_by(grouping_sets([vec!["tenant_id"], vec![]]))
            .select(diesel::dsl::sql::<(
                diesel::sql_types::Text,
                diesel::sql_types::BigInt,
            )>("tenant_id, count()"))
            .order(diesel::dsl::sql::<diesel::sql_types::Text>("tenant_id").asc()),
    )?;
    let grouping_sets_rows: Vec<(String, u64)> =
        fixture.client.query(&grouping_sets_sql).fetch_all().await?;
    assert_eq!(
        grouping_sets_rows,
        vec![
            (String::new(), 6),
            ("acme".to_string(), 3),
            ("beta".to_string(), 3),
        ]
    );

    let fill_sql = to_sql(
        &events
            .select(id)
            .filter(id.le(3))
            .order(with_fill(id).from(1_i64).to(5_i64).step(1_i64)),
    )?;
    let filled_numbers: Vec<u64> = fixture
        .client
        .query(&fill_sql)
        .bind(3_i64)
        .bind(1_i64)
        .bind(5_i64)
        .bind(1_i64)
        .fetch_all()
        .await?;
    assert_eq!(filled_numbers, vec![1, 2, 3, 4]);

    let sampled_sql = to_sql(
        &sample_offset(events, 1.0, 0.0)
            .select(diesel::dsl::sql::<diesel::sql_types::BigInt>("count()")),
    )?;
    let sampled_count: u64 = fixture
        .client
        .query(&sampled_sql)
        .bind(1.0_f64)
        .bind(0.0_f64)
        .fetch_one()
        .await?;
    assert_eq!(sampled_count, 6);

    let with_ties_sql = to_sql(&events.select(id).order(success.asc()).limit(1).with_ties())?;
    let mut tied_ids: Vec<u64> = fixture
        .client
        .query(&with_ties_sql)
        .bind(1_i64)
        .fetch_all()
        .await?;
    tied_ids.sort();
    assert_eq!(tied_ids, vec![2, 6]);

    let latest_per_tenant_sql = to_sql(
        &events.select((tenant_id, id)).qualify(
            row_number()
                .over_ch(partition_by(tenant_id).order_by(id.desc()))
                .eq(1_i64),
        ),
    )?;
    let mut latest_per_tenant: Vec<(String, u64)> = fixture
        .client
        .query(&latest_per_tenant_sql)
        .bind(1_i64)
        .fetch_all()
        .await?;
    latest_per_tenant.sort();
    assert_eq!(
        latest_per_tenant,
        vec![("acme".to_string(), 3), ("beta".to_string(), 6)]
    );

    let window_sql = to_sql(
        &events
            .filter(tenant_id.eq("acme"))
            .select((
                id,
                rank().over_window("by_latency"),
                dense_rank().over_window("by_latency"),
                lag_in_frame(latency_ms, 1_i64, 0.0).over_ch(
                    partition_by(tenant_id)
                        .order_by(id.asc())
                        .rows_between_unbounded_preceding_and_current_row(),
                ),
            ))
            .window(
                "by_latency",
                partition_by(tenant_id).order_by(latency_ms.asc()),
            ),
    )?;
    let mut window_rows: Vec<(u64, i64, i64, f64)> = fixture
        .client
        .query(&window_sql)
        .bind(1_i64)
        .bind(0.0_f64)
        .bind("acme")
        .fetch_all()
        .await?;
    window_rows.sort_by_key(|row| row.0);
    assert_eq!(
        window_rows,
        vec![(1, 1, 1, 0.0), (2, 2, 2, 10.0), (3, 3, 3, 20.0)]
    );

    let rows_frame_sql = to_sql(
        &events
            .select((
                tenant_id,
                id,
                diesel::dsl::sql::<diesel::sql_types::Double>("sum(`latency_ms`)").over_ch(
                    partition_by(tenant_id)
                        .order_by(id.asc())
                        .rows_between_preceding_and_following(1, 1),
                ),
            ))
            .order(tenant_id.asc())
            .then_order_by(id.asc()),
    )?;
    let rows_frame_rows: Vec<(String, u64, f64)> =
        fixture.client.query(&rows_frame_sql).fetch_all().await?;
    assert_eq!(
        rows_frame_rows,
        vec![
            ("acme".to_string(), 1, 30.0),
            ("acme".to_string(), 2, 60.0),
            ("acme".to_string(), 3, 50.0),
            ("beta".to_string(), 4, 90.0),
            ("beta".to_string(), 5, 150.0),
            ("beta".to_string(), 6, 110.0),
        ]
    );

    let range_frame_sql = to_sql(
        &events
            .filter(tenant_id.eq("acme"))
            .select((
                id,
                diesel::dsl::sql::<diesel::sql_types::Double>("sum(`latency_ms`)").over_ch(
                    partition_by(tenant_id)
                        .order_by(id.asc())
                        .range_between_preceding_and_current_row(1),
                ),
            ))
            .order(id.asc()),
    )?;
    let range_frame_rows: Vec<(u64, f64)> = fixture
        .client
        .query(&range_frame_sql)
        .bind("acme")
        .fetch_all()
        .await?;
    assert_eq!(range_frame_rows, vec![(1, 10.0), (2, 30.0), (3, 50.0)]);

    // LIMIT BY + SETTINGS compose after regular Diesel select/order clauses and
    // remain valid when clickhouse-rs appends its RowBinary FORMAT clause.
    let limit_by_query = events
        .select((tenant_id, id))
        .order(tenant_id.asc())
        .then_order_by(id.asc())
        .limit_by_col(2, "tenant_id")
        .settings([Setting::new("max_threads", 1)]);
    let limit_by_sql = to_sql(&limit_by_query)?;
    let rows: Vec<(String, u64)> = fixture.client.query(&limit_by_sql).fetch_all().await?;
    assert_eq!(
        rows,
        vec![
            ("acme".to_string(), 1),
            ("acme".to_string(), 2),
            ("beta".to_string(), 4),
            ("beta".to_string(), 5),
        ]
    );

    let scalar_sql = to_sql(&events.filter(id.eq(1)).select((
        length(tags),
        map_contains(attrs, "country"),
        json_extract_int(payload, "score"),
        date_diff(
            "hour",
            created_at,
            to_date_time(diesel::dsl::sql::<diesel::sql_types::Text>(
                "'2024-01-01 02:10:00'",
            )),
        ),
    )))?;
    let scalar_row: (u64, bool, i64, i64) = fixture
        .client
        .query(&scalar_sql)
        .bind("country")
        .bind("score")
        .bind("hour")
        .bind(1_i64)
        .fetch_one()
        .await?;
    assert_eq!(scalar_row, (2, true, 10, 2));

    let cast_sql = to_sql(&diesel::select((
        to_int32(diesel::dsl::sql::<diesel::sql_types::Text>("'42'")),
        to_uint64(is_null(to_int32_or_null(diesel::dsl::sql::<
            diesel::sql_types::Text,
        >("'oops'")))),
        to_uint64(is_null(to_uint64_or_null(diesel::dsl::sql::<
            diesel::sql_types::Text,
        >("'123'")))),
        to_uint64(is_null(to_float64_or_null(diesel::dsl::sql::<
            diesel::sql_types::Text,
        >("'abc'")))),
        cast::<diesel_clickhouse::sql_types::UInt64, _>(
            diesel::dsl::sql::<diesel::sql_types::Text>("'42'"),
            "UInt64",
        ),
        to_uint64(is_null(accurate_cast_or_null::<
            diesel_clickhouse::sql_types::UInt8,
            _,
        >(
            diesel::dsl::sql::<diesel::sql_types::Text>("'300'"),
            "UInt8",
        ))),
    )))?;
    let cast_row: (i32, u64, u64, u64, u64, u64) =
        fixture.client.query(&cast_sql).fetch_one().await?;
    assert_eq!(cast_row, (42, 1, 0, 1, 42, 1));

    let json_variant_sql = to_sql(&events.filter(id.eq(1)).select((
        json_extract_string_path(payload, ["country"]),
        json_extract_int_path(payload, ["score"]),
        to_uint64(json_has(payload, "country")),
        json_length(payload),
        json_value(payload, "$.score"),
        to_uint64(is_valid_json(payload)),
    )))?;
    let json_variant_row: (String, i64, u64, u64, String, u64) = fixture
        .client
        .query(&json_variant_sql)
        .bind("country")
        .bind("$.score")
        .bind(1_i64)
        .fetch_one()
        .await?;
    assert_eq!(
        json_variant_row,
        ("US".to_string(), 10, 1, 2, "10".to_string(), 1)
    );

    let simple_json_doc =
        diesel::dsl::sql::<diesel::sql_types::Text>("'{\"country\":\"US\",\"score\":10}'");
    let simple_json_sql = to_sql(&diesel::select((
        simple_json_extract_string(
            simple_json_doc.clone(),
            diesel::dsl::sql::<diesel::sql_types::Text>("'country'"),
        ),
        simple_json_extract_int(
            simple_json_doc.clone(),
            diesel::dsl::sql::<diesel::sql_types::Text>("'score'"),
        ),
        to_uint64(simple_json_has(
            simple_json_doc,
            diesel::dsl::sql::<diesel::sql_types::Text>("'score'"),
        )),
    )))?;
    let simple_json_row: (String, i64, u64) =
        fixture.client.query(&simple_json_sql).fetch_one().await?;
    assert_eq!(simple_json_row, ("US".to_string(), 10, 1));

    let higher_order_sql = to_sql(&events.filter(id.eq(1)).select((
        to_string(array_map(lambda("tag", "upper(tag)"), tags)),
        to_string(array_filter(lambda("tag", "tag = 'paid'"), tags)),
        to_uint64(array_exists(lambda("tag", "tag = 'mobile'"), tags)),
        to_uint64(array_count(lambda("tag", "tag = 'paid'"), tags)),
        to_uint64(map_contains(
            map_filter(lambda2("k", "v", "k = 'country'"), attrs),
            "country",
        )),
        to_string(map_apply(lambda2("k", "v", "(k, upper(v))"), attrs)),
    )))?;
    let higher_order_row: (String, String, u64, u64, u64, String) = fixture
        .client
        .query(&higher_order_sql)
        .bind("country")
        .bind(1_i64)
        .fetch_one()
        .await?;
    assert_eq!(higher_order_row.0, "['PAID','MOBILE']");
    assert_eq!(higher_order_row.1, "['paid']");
    assert_eq!(higher_order_row.2, 1);
    assert_eq!(higher_order_row.3, 1);
    assert_eq!(higher_order_row.4, 1);
    assert!(higher_order_row.5.contains("'country':'US'"));
    assert!(higher_order_row.5.contains("'plan':'PRO'"));

    let string_sql = to_sql(&events.filter(id.eq(1)).select((
        lower(tenant_id),
        upper(tenant_id),
        substring(tenant_id, 1_i64, 2_i64),
        position(tenant_id, "cm"),
        replace_all(tenant_id, "ac", "AC"),
        concat(tenant_id, "!"),
        regexp_match(tenant_id, "^ac"),
    )))?;
    let string_row: (String, String, String, u64, String, String, bool) = fixture
        .client
        .query(&string_sql)
        .bind(1_i64)
        .bind(2_i64)
        .bind("cm")
        .bind("ac")
        .bind("AC")
        .bind("!")
        .bind("^ac")
        .bind(1_i64)
        .fetch_one()
        .await?;
    assert_eq!(
        string_row,
        (
            "acme".to_string(),
            "ACME".to_string(),
            "ac".to_string(),
            2,
            "ACme".to_string(),
            "acme!".to_string(),
            true,
        )
    );

    let pattern_array = || {
        diesel::dsl::sql::<diesel_clickhouse::sql_types::Array<diesel::sql_types::Text>>(
            "['^ac', '^beta']",
        )
    };
    let search_sql = to_sql(&events.filter(id.eq(1)).select((
        tenant_id.like("ac%"),
        tenant_id.ilike("%AC%"),
        ilike(tenant_id, "%AC%"),
        multi_match_any(tenant_id, pattern_array()),
        multi_match_any_index(tenant_id, pattern_array()),
    )))?;
    let search_row: (bool, bool, bool, bool, u64) = fixture
        .client
        .query(&search_sql)
        .bind("ac%")
        .bind("%AC%")
        .bind("%AC%")
        .bind(1_i64)
        .fetch_one()
        .await?;
    assert_eq!(search_row, (true, true, true, true, 1));

    let url_value = "https://www.example.com/path/to?q=1#frag";
    let url_sql = to_sql(&diesel::select((
        domain(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        domain_without_www(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        top_level_domain(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        first_significant_subdomain(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        url_path(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        url_query_string(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        url_fragment(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        url_protocol(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
    )))?;
    let url_row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = fixture
        .client
        .query(&url_sql)
        .bind(url_value)
        .bind(url_value)
        .bind(url_value)
        .bind(url_value)
        .bind(url_value)
        .bind(url_value)
        .bind(url_value)
        .bind(url_value)
        .fetch_one()
        .await?;
    assert_eq!(
        url_row,
        (
            "www.example.com".to_string(),
            "example.com".to_string(),
            "com".to_string(),
            "example".to_string(),
            "/path/to".to_string(),
            "q=1".to_string(),
            "frag".to_string(),
            "https".to_string(),
        )
    );

    let url_extra_sql = to_sql(&diesel::select((
        url_path_full(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        cut_query_string(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
    )))?;
    let url_extra_row: (String, String) = fixture
        .client
        .query(&url_extra_sql)
        .bind(url_value)
        .bind(url_value)
        .fetch_one()
        .await?;
    assert_eq!(
        url_extra_row,
        (
            "/path/to?q=1#frag".to_string(),
            "https://www.example.com/path/to#frag".to_string(),
        )
    );

    let encoding_hash_sql = to_sql(&diesel::select((
        hex(diesel::dsl::sql::<diesel::sql_types::Text>("'A'")),
        unhex(diesel::dsl::sql::<diesel::sql_types::Text>("'41'")),
        base64_encode(diesel::dsl::sql::<diesel::sql_types::Text>("'click'")),
        base64_decode(diesel::dsl::sql::<diesel::sql_types::Text>("'Y2xpY2s='")),
        to_string(try_base64_decode(
            diesel::dsl::sql::<diesel::sql_types::Text>("'Y2xpY2s='"),
        )),
        city_hash64(diesel::dsl::sql::<diesel::sql_types::Text>("'click'")),
        sip_hash64(diesel::dsl::sql::<diesel::sql_types::Text>("'click'")),
        xx_hash64(diesel::dsl::sql::<diesel::sql_types::Text>("'click'")),
        farm_fingerprint64(diesel::dsl::sql::<diesel::sql_types::Text>("'click'")),
    )))?;
    let encoding_hash_row: (String, String, String, String, String, u64, u64, u64, u64) =
        fixture.client.query(&encoding_hash_sql).fetch_one().await?;
    assert_eq!(encoding_hash_row.0, "41");
    assert_eq!(encoding_hash_row.1, "A");
    assert_eq!(encoding_hash_row.2, "Y2xpY2s=");
    assert_eq!(encoding_hash_row.3, "click");
    assert_eq!(encoding_hash_row.4, "click");
    assert!(encoding_hash_row.5 > 0);
    assert!(encoding_hash_row.6 > 0);
    assert!(encoding_hash_row.7 > 0);
    assert!(encoding_hash_row.8 > 0);

    let ip_sql = to_sql(&diesel::select((
        to_string(to_ipv4(diesel::dsl::sql::<diesel::sql_types::Text>(
            "'127.0.0.1'",
        ))),
        ipv4_num_to_string(ipv4_string_to_num(diesel::dsl::sql::<
            diesel::sql_types::Text,
        >("'127.0.0.1'"))),
        to_string(to_ipv6(diesel::dsl::sql::<diesel::sql_types::Text>(
            "'2001:db8::1'",
        ))),
        ipv6_num_to_string(to_ipv6(diesel::dsl::sql::<diesel::sql_types::Text>(
            "'2001:db8::1'",
        ))),
        is_ipv4_string(diesel::dsl::sql::<diesel::sql_types::Text>("'127.0.0.1'")),
        is_ipv6_string(diesel::dsl::sql::<diesel::sql_types::Text>("'2001:db8::1'")),
    )))?;
    let ip_row: (String, String, String, String, bool, bool) =
        fixture.client.query(&ip_sql).fetch_one().await?;
    assert_eq!(
        ip_row,
        (
            "127.0.0.1".to_string(),
            "127.0.0.1".to_string(),
            "2001:db8::1".to_string(),
            "2001:db8::1".to_string(),
            true,
            true,
        )
    );

    let conversion_sql = to_sql(&events.filter(id.eq(1)).select((
        to_uint64(latency_ms),
        to_int64(success),
        to_float64(id),
        to_string(id),
    )))?;
    let conversion_row: (u64, i64, f64, String) = fixture
        .client
        .query(&conversion_sql)
        .bind(1_i64)
        .fetch_one()
        .await?;
    assert_eq!(conversion_row, (10, 1, 1.0, "1".to_string()));

    let numeric_sql = to_sql(&events.filter(id.eq(2)).select((
        abs(latency_ms),
        round(latency_ms),
        floor(latency_ms),
        ceil(latency_ms),
        least(latency_ms, 15.0),
        greatest(latency_ms, 25.0),
    )))?;
    let numeric_row: (f64, f64, f64, f64, f64, f64) = fixture
        .client
        .query(&numeric_sql)
        .bind(15.0_f64)
        .bind(25.0_f64)
        .bind(2_i64)
        .fetch_one()
        .await?;
    assert_eq!(numeric_row, (20.0, 20.0, 20.0, 20.0, 15.0, 25.0));

    let tag_source = events.array_join_as(tags, "tag");
    let tag_sql = to_sql(&tag_source.select(diesel::dsl::sql::<diesel::sql_types::Text>("tag")))?;
    let mut tag_rows: Vec<String> = fixture.client.query(&tag_sql).fetch_all().await?;
    tag_rows.sort();
    assert_eq!(
        tag_rows,
        vec![
            "desktop".to_string(),
            "mobile".to_string(),
            "mobile".to_string(),
            "paid".to_string(),
            "paid".to_string(),
            "paid".to_string(),
            "trial".to_string(),
            "trial".to_string(),
        ]
    );

    // PREWHERE and FINAL must appear in the FROM section, before WHERE. Source
    // wrappers currently require raw select expressions because Diesel's table
    // macro only marks columns as selectable from the original table and common
    // built-in wrappers.
    let prewhere_source = prewhere(final_table(events), tenant_id.eq("beta"));
    let prewhere_query =
        prewhere_source.select(diesel::dsl::sql::<diesel::sql_types::BigInt>("count()"));
    let prewhere_sql = to_sql(&prewhere_query)?;
    let beta_count: u64 = fixture
        .client
        .query(&prewhere_sql)
        .bind("beta")
        .fetch_one()
        .await?;
    assert_eq!(beta_count, 3);

    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_type_showcase")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_projection_rollups")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_summing_rollups")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_mutations")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_partitions")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_image_vectors")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_mv")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_mv_target")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_mv_source")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_aggregate_states")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_ddl_events")
        .execute()
        .await?;
    fixture
        .client
        .query("DROP TABLE diesel_clickhouse_events")
        .execute()
        .await?;
    Ok(())
}
