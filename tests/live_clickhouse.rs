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

use diesel::prelude::*;
use testcontainers_modules::{
    clickhouse::{CLICKHOUSE_PORT, ClickHouse as ClickHouseImage},
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

use diesel_clickhouse::{
    ClickHouseJoinDsl, ClickHouseQueryDsl, ClickHouseTextExpressionMethods, Column, DataType,
    NestedField, OverDsl, Setting, TableEngine, TableIndex, abs, accurate_cast_or_null, aggregate,
    aggregating_merge_tree, alter_table, analysis_of_variance, approx_top_sum, array_count,
    array_exists, array_filter, array_map, base64_decode, base64_encode, cast, ceil, city_hash64,
    concat, corr, count_if, count_merge, covar_pop, covar_pop_stable, covar_samp,
    covar_samp_stable, create_materialized_view, create_table, cut_query_string, date_diff,
    dense_rank, domain, domain_without_www, farm_fingerprint64, final_table, finalize_aggregation,
    first_significant_subdomain, floor, greatest, group_by_all, grouping_sets, hex, histogram,
    ilike, ipv4_num_to_string, ipv4_string_to_num, ipv6_num_to_string, is_ipv4_string,
    is_ipv6_string, is_null, is_valid_json, json_extract_int, json_extract_int_path,
    json_extract_string_path, json_has, json_length, json_value, l2_distance, lag_in_frame, lambda,
    lambda2, least, length, lower, mann_whitney_u_test, map_apply, map_contains, map_filter,
    max_if, merge_tree, min_if, multi_match_any, multi_match_any_index, mutation_assignment,
    partition_by, partition_expr, position, prewhere, projection, quantile, quantile_deterministic,
    quantile_exact, quantile_timing, quantiles, quantiles_timing, rank, regexp_match, replace_all,
    replacing_merge_tree, rollup, round, row_number, sample_offset, simple_json_extract_int,
    simple_json_extract_string, simple_json_has, sip_hash64, stddev_pop, stddev_pop_stable,
    stddev_samp, substring, sum_merge, sum_state, summing_merge_tree, to_date_time, to_float64,
    to_float64_or_null, to_int32, to_int32_or_null, to_int64, to_ipv4, to_ipv6, to_sql, to_string,
    to_uint64, to_uint64_or_null, top_k, top_level_domain, try_base64_decode, unhex, uniq_exact_if,
    uniq_exact_merge, upper, url_fragment, url_path, url_path_full, url_protocol, url_query_string,
    var_pop, var_pop_stable, vector_f32, with_fill, xx_hash64,
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

diesel::joinable!(events -> tenants (tenant_id));
diesel::allow_tables_to_appear_in_same_query!(
    events,
    tenants,
    tenant_rates,
    aggregate_states,
    mv_source,
    image_vectors
);

struct ClickHouseFixture {
    _node: ContainerAsync<ClickHouseImage>,
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
    let client = clickhouse::Client::default()
        .with_url(url)
        .with_user("default")
        .with_password("password");
    Ok(ClickHouseFixture {
        _node: node,
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
        .with_option("allow_experimental_dynamic_type", "1")
        .with_option("allow_experimental_variant_type", "1")
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
                .over(partition_by(tenant_id).order_by(id.desc()))
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
                lag_in_frame(latency_ms, 1_i64, 0.0).over(
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
                diesel::dsl::sql::<diesel::sql_types::Double>("sum(`latency_ms`)").over(
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
                diesel::dsl::sql::<diesel::sql_types::Double>("sum(`latency_ms`)").over(
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
