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
    ClickHouseQueryDsl, OverDsl, Setting, abs, ceil, concat, count_if, date_diff, dense_rank,
    final_table, floor, greatest, group_by_all, grouping_sets, json_extract_int, lag_in_frame,
    least, length, lower, map_contains, max_if, min_if, partition_by, position, prewhere, quantile,
    quantile_exact, quantiles, rank, regexp_match, replace_all, rollup, round, row_number,
    sample_offset, substring, to_date_time, to_float64, to_int64, to_sql, to_string, to_uint64,
    top_k, uniq_exact_if, upper, with_fill,
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

    let quantiles_sql = to_sql(&events.select(quantiles([0.25, 0.5, 0.75], latency_ms)))?;
    let latency_quantiles: Vec<f64> = fixture.client.query(&quantiles_sql).fetch_one().await?;
    assert_eq!(latency_quantiles.len(), 3);

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
        .query("DROP TABLE diesel_clickhouse_events")
        .execute()
        .await?;
    Ok(())
}
