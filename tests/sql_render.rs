use diesel::prelude::*;
use diesel_clickhouse::{
    ClickHouseQueryDsl, Format, OverDsl, Setting, abs, ceil, concat, count_if, cube, date_diff,
    dense_rank, final_table, floor, greatest, group_by_all, grouping, grouping_sets, if_,
    json_extract_int, lag_in_frame, least, length, lower, map_contains, partition_by, position,
    prewhere, quantile, quantile_exact, quantiles, rank, regexp_match, replace_all, rollup, round,
    row_number, sample, sample_offset, substring, to_date_time, to_float64, to_int64, to_sql,
    to_start_of_hour, to_string, to_uint64, top_k, upper, with_fill, with_totals,
};

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

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

#[test]
fn renders_clickhouse_functions_and_trailing_clauses() {
    use self::events::dsl::*;

    let query = events
        .filter(tenant_id.eq("acme"))
        .group_by(tenant_id)
        .select((tenant_id, count_if(success), quantile(0.95, latency_ms)))
        .order(tenant_id.desc())
        .limit_by_col(10, "tenant_id")
        .format(Format::JsonEachRow);

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `events`.`tenant_id`, countIf(`events`.`success`), quantile(0.95)(`events`.`latency_ms`) FROM `events` WHERE (`events`.`tenant_id` = ?) GROUP BY `events`.`tenant_id` ORDER BY `events`.`tenant_id` DESC LIMIT 10 BY `tenant_id` FORMAT JSONEachRow"
    );
}

#[test]
fn renders_clickhouse_scalar_functions() {
    use self::events::dsl::*;

    let query = events.select((
        to_start_of_hour(created_at),
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
        if_(success, latency_ms, 0.0),
    ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT toStartOfHour(`events`.`created_at`), length(`events`.`tags`), mapContains(`events`.`attrs`, ?), JSONExtractInt(`events`.`payload`, ?), dateDiff(?, `events`.`created_at`, toDateTime('2024-01-01 02:10:00')), if(`events`.`success`, `events`.`latency_ms`, ?) FROM `events`"
    );
}

#[test]
fn renders_source_modifiers_before_where() {
    use self::events::dsl::*;

    let source = prewhere(sample(final_table(events), 0.1), tenant_id.eq("acme"));
    let query = source
        .select(diesel::dsl::sql::<diesel::sql_types::BigInt>(
            "`events`.`id`",
        ))
        .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
            "`events`.`success` = 1",
        ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `events`.`id` FROM `events` FINAL SAMPLE ? PREWHERE (`events`.`tenant_id` = ?) WHERE `events`.`success` = 1"
    );
}

#[test]
fn renders_array_join_clause() {
    use self::events::dsl::*;

    let source = final_table(events).array_join_as(tags, "tag");
    let query = source
        .select(diesel::dsl::sql::<diesel::sql_types::Text>("`tag`"))
        .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
            "`tag` = 'paid'",
        ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `tag` FROM `events` FINAL ARRAY JOIN `events`.`tags` AS `tag` WHERE `tag` = 'paid'"
    );
}

#[test]
fn renders_string_numeric_and_conversion_functions() {
    use self::events::dsl::*;

    let string_query = events.select((
        lower(tenant_id),
        upper(tenant_id),
        substring(tenant_id, 1_i64, 2_i64),
        position(tenant_id, "cm"),
        replace_all(tenant_id, "ac", "AC"),
        concat(tenant_id, payload),
        regexp_match(tenant_id, "^ac"),
        to_uint64(latency_ms),
        to_int64(success),
        to_float64(id),
        to_string(id),
    ));
    let numeric_query = events.select((
        abs(latency_ms),
        round(latency_ms),
        floor(latency_ms),
        ceil(latency_ms),
        least(latency_ms, 20.0),
        greatest(latency_ms, 20.0),
    ));

    assert_eq!(
        to_sql(&string_query).unwrap(),
        "SELECT lower(`events`.`tenant_id`), upper(`events`.`tenant_id`), substring(`events`.`tenant_id`, ?, ?), position(`events`.`tenant_id`, ?), replaceAll(`events`.`tenant_id`, ?, ?), concat(`events`.`tenant_id`, `events`.`payload`), match(`events`.`tenant_id`, ?), toUInt64(`events`.`latency_ms`), toInt64(`events`.`success`), toFloat64(`events`.`id`), toString(`events`.`id`) FROM `events`"
    );
    assert_eq!(
        to_sql(&numeric_query).unwrap(),
        "SELECT abs(`events`.`latency_ms`), round(`events`.`latency_ms`), floor(`events`.`latency_ms`), ceil(`events`.`latency_ms`), least(`events`.`latency_ms`, ?), greatest(`events`.`latency_ms`, ?) FROM `events`"
    );
}

#[test]
fn renders_parametric_aggregates() {
    use self::events::dsl::*;

    let query = events.select((
        quantile_exact(0.5, latency_ms),
        quantiles([0.25, 0.75], latency_ms),
        top_k(3, tenant_id),
    ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT quantileExact(0.5)(`events`.`latency_ms`), quantiles(0.25, 0.75)(`events`.`latency_ms`), topK(3)(`events`.`tenant_id`) FROM `events`"
    );
}

#[test]
fn renders_group_by_modifiers() {
    use self::events::dsl::*;

    let totals_query = events
        .group_by(with_totals(rollup((tenant_id, success))))
        .select(count_if(success));
    let cube_query = events
        .group_by(cube((tenant_id, success)))
        .select(count_if(success));
    let all_query = events.group_by(group_by_all()).select(diesel::dsl::sql::<(
        diesel::sql_types::Text,
        diesel::sql_types::BigInt,
    )>("tenant_id, count()"));
    let grouping_sets_query = events
        .group_by(grouping_sets([
            vec!["tenant_id", "success"],
            vec!["tenant_id"],
            vec![],
        ]))
        .select(diesel::dsl::sql::<diesel::sql_types::BigInt>("count()"));
    let grouping_fn_query = events.select(grouping((tenant_id, success)));

    assert_eq!(
        to_sql(&totals_query).unwrap(),
        "SELECT countIf(`events`.`success`) FROM `events` GROUP BY ROLLUP(`events`.`tenant_id`, `events`.`success`) WITH TOTALS"
    );
    assert_eq!(
        to_sql(&cube_query).unwrap(),
        "SELECT countIf(`events`.`success`) FROM `events` GROUP BY CUBE(`events`.`tenant_id`, `events`.`success`)"
    );
    assert_eq!(
        to_sql(&all_query).unwrap(),
        "SELECT tenant_id, count() FROM `events` GROUP BY ALL"
    );
    assert_eq!(
        to_sql(&grouping_sets_query).unwrap(),
        "SELECT count() FROM `events` GROUP BY GROUPING SETS ((`tenant_id`, `success`), (`tenant_id`), ())"
    );
    assert_eq!(
        to_sql(&grouping_fn_query).unwrap(),
        "SELECT GROUPING(`events`.`tenant_id`, `events`.`success`) FROM `events`"
    );
}

#[test]
fn renders_with_aliases_sample_offset_and_limit_ties() {
    use self::events::dsl::*;

    let with_query = diesel::select(diesel::dsl::sql::<diesel::sql_types::BigInt>("next_answer"))
        .with_alias(
            diesel::dsl::sql::<diesel::sql_types::BigInt>("toInt64(41)"),
            "answer",
        )
        .and_with_alias(
            diesel::dsl::sql::<diesel::sql_types::BigInt>("answer + toInt64(1)"),
            "next_answer",
        );
    let cte_query = diesel::select(diesel::dsl::sql::<diesel::sql_types::BigInt>(
        "x FROM answer_cte",
    ))
    .with_cte(
        "answer_cte",
        diesel::select(diesel::dsl::sql::<diesel::sql_types::BigInt>(
            "toInt64(42) AS x",
        )),
    );
    let sampled_query = sample_offset(events, 1.0, 0.0)
        .select(diesel::dsl::sql::<diesel::sql_types::BigInt>("count()"));
    let with_ties_query = events.select(id).order(success.asc()).limit(1).with_ties();
    let fill_query = events
        .select(id)
        .order(with_fill(id).from(1_i64).to(10_i64).step(1_i64));

    assert_eq!(
        to_sql(&with_query).unwrap(),
        "WITH toInt64(41) AS `answer`, answer + toInt64(1) AS `next_answer` SELECT next_answer"
    );
    assert_eq!(
        to_sql(&cte_query).unwrap(),
        "WITH `answer_cte` AS (SELECT toInt64(42) AS x) SELECT x FROM answer_cte"
    );
    assert_eq!(
        to_sql(&sampled_query).unwrap(),
        "SELECT count() FROM `events` SAMPLE ? OFFSET ?"
    );
    assert_eq!(
        to_sql(&with_ties_query).unwrap(),
        "SELECT `events`.`id` FROM `events` ORDER BY `events`.`success` ASC LIMIT ? WITH TIES"
    );
    assert_eq!(
        to_sql(&fill_query).unwrap(),
        "SELECT `events`.`id` FROM `events` ORDER BY `events`.`id` WITH FILL FROM ? TO ? STEP ?"
    );
}

#[test]
fn renders_window_functions_qualify_and_named_windows() {
    use self::events::dsl::*;

    let qualify_query = events.select((tenant_id, id)).qualify(
        row_number()
            .over(partition_by(tenant_id).order_by(id.desc()))
            .eq(1_i64),
    );
    let named_window_query = events
        .select((
            tenant_id,
            rank().over_window("by_tenant"),
            dense_rank().over(partition_by(tenant_id).order_by(latency_ms.desc())),
            lag_in_frame(latency_ms, 1_i64, 0.0).over(
                partition_by(tenant_id)
                    .order_by(id.asc())
                    .rows_between_unbounded_preceding_and_current_row(),
            ),
        ))
        .window(
            "by_tenant",
            partition_by(tenant_id)
                .order_by(latency_ms.desc())
                .rows_between_unbounded_preceding_and_current_row(),
        );

    assert_eq!(
        to_sql(&qualify_query).unwrap(),
        "SELECT `events`.`tenant_id`, `events`.`id` FROM `events` QUALIFY (row_number() OVER (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`id` DESC) = ?)"
    );
    assert_eq!(
        to_sql(&named_window_query).unwrap(),
        "SELECT `events`.`tenant_id`, rank() OVER `by_tenant`, dense_rank() OVER (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`latency_ms` DESC), lagInFrame(`events`.`latency_ms`, ?, ?) OVER (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`id` ASC ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM `events` WINDOW `by_tenant` AS (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`latency_ms` DESC ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)"
    );
}

#[test]
fn renders_settings() {
    use self::events::dsl::*;

    let query = events
        .select(id)
        .settings([Setting::new("max_threads", 4), Setting::flag("readonly")]);

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `events`.`id` FROM `events` SETTINGS max_threads = 4, readonly"
    );
}
