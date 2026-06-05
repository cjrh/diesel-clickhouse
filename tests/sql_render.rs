use diesel::prelude::*;
use diesel_clickhouse::{
    ClickHouseJoinDsl, ClickHouseQueryDsl, ClickHouseTextExpressionMethods, Column, DataType,
    Format, NamedParameterMetadata, NestedField, OutfileCompression, OverDsl, Setting, TableEngine,
    TableIndex, VectorDistanceFunction, VectorQuantization, WindowFrameBound, abs, accurate_cast,
    accurate_cast_or_default, accurate_cast_or_null, aggregate, aggregating_merge_tree, alias_ref,
    alias_source, alter_table, analysis_of_variance, analyze_rendered_sql, approx_top_sum,
    approx_top_sum_with_reserved, array_all, array_count, array_exists, array_exists2,
    array_filter, array_map, avg_merge, avg_merge_state, avg_state, base64_decode, base64_encode,
    buffer, cast, ceil, city_hash64, collapsing_merge_tree, concat, corr, cosine_distance, count,
    count_if, count_merge, count_merge_state, count_state, covar_pop, covar_pop_stable, covar_samp,
    covar_samp_stable, create_materialized_view, create_table, cube, cut_query_string, date_diff,
    dense_rank, distributed, domain, domain_without_www, expr_as, farm_fingerprint64, final_table,
    finalize_aggregation, first_significant_subdomain, floor, greatest, group_array_merge,
    group_array_state, group_by_all, grouping, grouping_sets, hex, histogram, if_, ilike,
    ilike_escape, ipv4_num_to_string, ipv4_string_to_num, ipv6_num_to_string, is_ipv4_string,
    is_ipv6_string, is_valid_json, join_column, json_exists, json_extract_int, json_extract_int_ci,
    json_extract_int_ci_path, json_extract_int_path, json_extract_raw_ci, json_extract_raw_path,
    json_extract_string_ci, json_extract_string_path, json_has, json_length, json_query,
    json_value, l1_distance, l1_norm, l2_distance, l2_norm, lag_in_frame, lambda, lambda2, least,
    left_utf8, length, length_utf8, like, like_escape, linf_distance, linf_norm, lower,
    mann_whitney_u_test, map_apply, map_contains, map_filter, map_from_arrays, map_keys,
    map_values, max_merge, max_state, min_merge, min_state, multi_fuzzy_match_all_indices,
    multi_fuzzy_match_any, multi_fuzzy_match_any_index, multi_match_all_indices, multi_match_any,
    multi_match_any_index, mutation_assignment, not_ilike, not_ilike_escape, not_like,
    not_like_escape, null_if, partition_by, partition_expr, partition_id, position,
    position_case_insensitive, prewhere, projection, quantile, quantile_deterministic,
    quantile_exact, quantile_timing, quantiles, quantiles_timing, rank, regexp_match, replace_all,
    replacing_merge_tree, rollup, round, row_number, sample, sample_offset,
    simple_json_extract_int, simple_json_extract_string, simple_json_has, sip_hash64,
    source_column, source_column_as, stddev_pop, stddev_pop_stable, stddev_samp,
    stddev_samp_stable, substring, sum_merge, sum_merge_state, sum_state, summing_merge_tree_with,
    to_bool, to_date_time, to_float32, to_float64, to_float64_or_null, to_int32, to_int32_or_null,
    to_int64, to_int64_or_zero, to_int128, to_ipv4, to_ipv6, to_sql, to_sql_with_metadata,
    to_start_of_hour, to_string, to_uint32, to_uint64, to_uint64_or_null, to_uint64_or_zero,
    to_uint128, top_k, top_level_domain, try_base64_decode, unhex, uniq_exact_merge,
    uniq_exact_state, uniq_merge, uniq_state, upper, url_fragment, url_path, url_path_full,
    url_protocol, url_query_string, var_pop, var_pop_stable, var_samp, var_samp_stable, vector_f32,
    vector_f32_binary, vector_f32_hex, vector_f32_le_hex, vector_f64, vector_f64_hex,
    vector_similarity_index, versioned_collapsing_merge_tree, with_fill, with_totals, xx_hash64,
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

diesel::table! {
    tenants (tenant_id) {
        tenant_id -> Text,
        plan -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    image_vectors (id) {
        id -> BigInt,
        caption -> Text,
        embedding -> Array<Float>,
    }
}

diesel::table! {
    tenant_rates (tenant_id, effective_at) {
        tenant_id -> Text,
        effective_at -> Timestamp,
        rate -> Double,
    }
}

diesel::joinable!(events -> tenants (tenant_id));
diesel::allow_tables_to_appear_in_same_query!(events, tenants, tenant_rates, image_vectors);

// `treat_none_as_default_value = false` is required for `#[derive(Insertable)]`
// on ClickHouse: ClickHouse has no SQL `DEFAULT` keyword for `INSERT`, and the
// defaultable insert values Diesel emits by default only render on backends
// that support it (or via a backend-specific impl, which the orphan rule
// forbids third-party backends from writing).
#[derive(Insertable)]
#[diesel(table_name = tenants, treat_none_as_default_value = false)]
struct NewTenant<'a> {
    tenant_id: &'a str,
    plan: &'a str,
}

#[test]
fn renders_single_row_insert_from_tuple() {
    let stmt = diesel::insert_into(tenants::table)
        .values((tenants::tenant_id.eq("acme"), tenants::plan.eq("pro")));
    assert_eq!(
        to_sql(&stmt).unwrap(),
        "INSERT INTO `tenants` (`tenant_id`, `plan`) VALUES (?, ?)"
    );
}

#[test]
fn renders_single_row_insert_from_insertable_struct() {
    let row = NewTenant {
        tenant_id: "beta",
        plan: "free",
    };
    let stmt = diesel::insert_into(tenants::table).values(&row);
    assert_eq!(
        to_sql(&stmt).unwrap(),
        "INSERT INTO `tenants` (`tenant_id`, `plan`) VALUES (?, ?)"
    );
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
fn renders_diesel_native_clickhouse_features() {
    use self::events::dsl::*;

    let builtin_query = events
        .filter(
            tenant_id
                .eq("acme")
                .and(success.eq(true).or(latency_ms.gt(25.0))),
        )
        .group_by(tenant_id)
        .having(diesel::dsl::count_star().gt(1_i64))
        .select((tenant_id, diesel::dsl::count_star()))
        .order(tenant_id.asc())
        .limit(5)
        .offset(10);
    let nullable_query = diesel::select((
        diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Integer>>("NULL")
            .is_null(),
        diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Integer>>("NULL")
            .is_not_null(),
    ));
    let ansi_join_query = events
        .inner_join(tenants::table.on(tenant_id.eq(tenants::tenant_id)))
        .select((tenant_id, tenants::plan));

    assert_eq!(
        to_sql(&builtin_query).unwrap(),
        "SELECT `events`.`tenant_id`, COUNT(*) FROM `events` WHERE ((`events`.`tenant_id` = ?) AND ((`events`.`success` = ?) OR (`events`.`latency_ms` > ?))) GROUP BY `events`.`tenant_id` HAVING (COUNT(*) > ?) ORDER BY `events`.`tenant_id` ASC LIMIT ? OFFSET ?"
    );
    assert_eq!(
        to_sql(&nullable_query).unwrap(),
        "SELECT (NULL IS NULL), (NULL IS NOT NULL)"
    );
    assert_eq!(
        to_sql(&ansi_join_query).unwrap(),
        "SELECT `events`.`tenant_id`, `tenants`.`plan` FROM (`events` INNER JOIN `tenants` ON (`events`.`tenant_id` = `tenants`.`tenant_id`))"
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
        .select((source_column(id), source_column_as(tenant_id, "tenant")))
        .filter(success.eq(true));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `events`.`id`, `events`.`tenant_id` AS `tenant` FROM `events` FINAL SAMPLE ? PREWHERE (`events`.`tenant_id` = ?) WHERE (`events`.`success` = ?)"
    );
}

#[test]
fn renders_aliased_source_wrapper() {
    use self::events::dsl::*;

    let source = alias_source(final_table(events), "e");
    let query = source
        .select(diesel::dsl::sql::<diesel::sql_types::BigInt>("e.id AS id"))
        .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
            "e.tenant_id = ?",
        ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT e.id AS id FROM `events` FINAL AS `e` WHERE e.tenant_id = ?"
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
fn rendered_sql_reports_bind_metadata() {
    use self::events::dsl::*;

    type TextArray = diesel_clickhouse::sql_types::Array<diesel::sql_types::Text>;

    let query = events
        .select((
            id,
            diesel::dsl::sql::<diesel::sql_types::Text>("{bucket:String}"),
        ))
        .filter(tenant_id.eq("acme"))
        .filter(array_exists(
            lambda("tag", "tag = 'paid'"),
            diesel_clickhouse::bind::<TextArray, _>(vec!["paid".to_owned()]),
        ))
        .filter(diesel::dsl::sql::<diesel::sql_types::Bool>(
            "(has({allowed:Array(String)}, tags) OR has({allowed:Array(String)}, tags)) AND payload != '?'",
        ))
        .limit(5);
    let rendered = to_sql_with_metadata(&query).unwrap();

    assert_eq!(rendered.positional_bind_count(), 3);
    assert_eq!(
        rendered.positional_bind_types(),
        &["String", "Array(String)", "Int64"]
    );
    assert_eq!(rendered.named_parameters(), &["bucket", "allowed"]);
    assert_eq!(
        rendered.named_parameter_details(),
        &[
            NamedParameterMetadata {
                name: "bucket".to_owned(),
                type_name: "String".to_owned(),
                occurrences: 1,
            },
            NamedParameterMetadata {
                name: "allowed".to_owned(),
                type_name: "Array(String)".to_owned(),
                occurrences: 2,
            },
        ]
    );
}

#[test]
fn scanner_ignores_placeholders_inside_quotes_and_comments() {
    // Each non-Normal lexer state must hide `?` and `{name:Type}` from the
    // scan. Only the two trailing real placeholders should be counted.
    let sql = "SELECT 'a ? {x:Int}', \"b ? {y:Int}\", `c ? {z:Int}`, \
               -- d ? {w:Int}\n /* e ? {v:Int} */ ? {real:Int}";
    let meta = analyze_rendered_sql(sql);

    assert_eq!(meta.positional_bind_count, 1);
    assert!(meta.positional_bind_types.is_empty());
    assert_eq!(meta.named_parameters, vec!["real".to_string()]);
    assert_eq!(
        meta.named_parameter_details,
        vec![NamedParameterMetadata {
            name: "real".to_string(),
            type_name: "Int".to_string(),
            occurrences: 1,
        }]
    );
}

#[test]
fn scanner_handles_doubled_and_escaped_quotes() {
    // A doubled `''` is an escaped quote, not a string terminator, so the `?`
    // after it is still inside the string. The backslash escape behaves the
    // same. The only counted placeholder is the final one outside the string.
    let meta = analyze_rendered_sql("'it''s ? a \\' test' || 'x' || ?");

    assert_eq!(meta.positional_bind_count, 1);
    assert!(meta.positional_bind_types.is_empty());
    assert!(meta.named_parameters.is_empty());
    assert!(meta.named_parameter_details.is_empty());
}

#[test]
fn scanner_dedupes_named_parameters_in_first_seen_order() {
    let meta = analyze_rendered_sql("{b:Int} = {a:Int} AND {b:Int} = {a:Int} AND {b:String}");

    assert_eq!(
        meta.named_parameters,
        vec!["b".to_string(), "a".to_string()]
    );
    assert_eq!(
        meta.named_parameter_details,
        vec![
            NamedParameterMetadata {
                name: "b".to_string(),
                type_name: "Int".to_string(),
                occurrences: 2,
            },
            NamedParameterMetadata {
                name: "a".to_string(),
                type_name: "Int".to_string(),
                occurrences: 2,
            },
            NamedParameterMetadata {
                name: "b".to_string(),
                type_name: "String".to_string(),
                occurrences: 1,
            },
        ]
    );
}

#[test]
fn scanner_rejects_malformed_named_parameters() {
    // `{` that is not a well-formed `{name:Type}` parameter (digit-led name,
    // missing colon, unterminated) must not be reported as a parameter.
    let meta = analyze_rendered_sql("{1bad:Int} {nocolon} {open:Int {empty:}");

    assert!(meta.named_parameters.is_empty());
    assert!(meta.named_parameter_details.is_empty());
}

#[test]
fn aliased_source_method_form_matches_free_function() {
    use self::events::dsl::*;

    let query = final_table(events)
        .alias_source("e")
        .select(diesel::dsl::sql::<diesel::sql_types::BigInt>("e.id"));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT e.id FROM `events` FINAL AS `e`"
    );
}

#[test]
fn invalid_source_alias_is_rejected() {
    use self::events::dsl::*;

    let query = alias_source(final_table(events), "1bad")
        .select(diesel::dsl::sql::<diesel::sql_types::BigInt>("e.id"));

    assert!(to_sql(&query).is_err());
}

#[test]
fn invalid_column_alias_is_rejected() {
    use self::events::dsl::*;

    let query = events.select(source_column_as(id, "has space"));

    assert!(to_sql(&query).is_err());
}

#[test]
fn renders_and_validates_alias_references() {
    use self::events::dsl::*;

    let query = events
        .select(expr_as(latency_ms, "score"))
        .order(alias_ref::<diesel::sql_types::Double>("score").desc());
    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `events`.`latency_ms` AS `score` FROM `events` ORDER BY `score` DESC"
    );

    let invalid = events
        .select(expr_as(latency_ms, "score"))
        .order(alias_ref::<diesel::sql_types::Double>("bad alias").desc());
    assert!(to_sql(&invalid).is_err());
}

#[test]
fn renders_string_numeric_and_conversion_functions() {
    use self::events::dsl::*;

    let string_query = events.select((
        lower(tenant_id),
        upper(tenant_id),
        substring(tenant_id, 1_i64, 2_i64),
        left_utf8(tenant_id, 2_i64),
        length_utf8(tenant_id),
        position(tenant_id, "cm"),
        position_case_insensitive(tenant_id, "AC"),
        replace_all(tenant_id, "ac", "AC"),
        concat(tenant_id, payload),
        null_if(tenant_id, ""),
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
        "SELECT lower(`events`.`tenant_id`), upper(`events`.`tenant_id`), substring(`events`.`tenant_id`, ?, ?), leftUTF8(`events`.`tenant_id`, ?), lengthUTF8(`events`.`tenant_id`), position(`events`.`tenant_id`, ?), positionCaseInsensitive(`events`.`tenant_id`, ?), replaceAll(`events`.`tenant_id`, ?, ?), concat(`events`.`tenant_id`, `events`.`payload`), nullIf(`events`.`tenant_id`, ?), match(`events`.`tenant_id`, ?), toUInt64(`events`.`latency_ms`), toInt64(`events`.`success`), toFloat64(`events`.`id`), toString(`events`.`id`) FROM `events`"
    );
    assert_eq!(
        to_sql(&numeric_query).unwrap(),
        "SELECT abs(`events`.`latency_ms`), round(`events`.`latency_ms`), floor(`events`.`latency_ms`), ceil(`events`.`latency_ms`), least(`events`.`latency_ms`, ?), greatest(`events`.`latency_ms`, ?) FROM `events`"
    );
}

#[test]
fn renders_like_regex_and_multi_match_helpers() {
    use self::events::dsl::*;

    let pattern_array = || {
        diesel::dsl::sql::<diesel_clickhouse::sql_types::Array<diesel::sql_types::Text>>(
            "['^ac', 'beta$']",
        )
    };
    let like_query = events.select((
        tenant_id.like("ac%"),
        tenant_id.not_like("%z"),
        tenant_id.ilike("%AC%"),
        tenant_id.not_ilike("%BETA%"),
        like(tenant_id, "a%"),
        like_escape(tenant_id, "a!_%", "!"),
        not_like(tenant_id, "%z"),
        not_like_escape(tenant_id, "z!_%", "!"),
        ilike(tenant_id, "%AC%"),
        ilike_escape(tenant_id, "A!_%", "!"),
        not_ilike(tenant_id, "%BETA%"),
        not_ilike_escape(tenant_id, "B!_%", "!"),
    ));
    let match_query = events.select((
        regexp_match(tenant_id, "^ac"),
        multi_match_any(tenant_id, pattern_array()),
        multi_match_any_index(tenant_id, pattern_array()),
        multi_match_all_indices(tenant_id, pattern_array()),
        multi_fuzzy_match_any(tenant_id, 1_i32, pattern_array()),
        multi_fuzzy_match_any_index(tenant_id, 1_i32, pattern_array()),
        multi_fuzzy_match_all_indices(tenant_id, 1_i32, pattern_array()),
    ));

    assert_eq!(
        to_sql(&like_query).unwrap(),
        "SELECT (`events`.`tenant_id` LIKE ?), (`events`.`tenant_id` NOT LIKE ?), `events`.`tenant_id` ILIKE ?, `events`.`tenant_id` NOT ILIKE ?, like(`events`.`tenant_id`, ?), like(`events`.`tenant_id`, ?, ?), notLike(`events`.`tenant_id`, ?), notLike(`events`.`tenant_id`, ?, ?), ilike(`events`.`tenant_id`, ?), ilike(`events`.`tenant_id`, ?, ?), notILike(`events`.`tenant_id`, ?), notILike(`events`.`tenant_id`, ?, ?) FROM `events`"
    );
    assert_eq!(
        to_sql(&match_query).unwrap(),
        "SELECT match(`events`.`tenant_id`, ?), multiMatchAny(`events`.`tenant_id`, ['^ac', 'beta$']), multiMatchAnyIndex(`events`.`tenant_id`, ['^ac', 'beta$']), multiMatchAllIndices(`events`.`tenant_id`, ['^ac', 'beta$']), multiFuzzyMatchAny(`events`.`tenant_id`, ?, ['^ac', 'beta$']), multiFuzzyMatchAnyIndex(`events`.`tenant_id`, ?, ['^ac', 'beta$']), multiFuzzyMatchAllIndices(`events`.`tenant_id`, ?, ['^ac', 'beta$']) FROM `events`"
    );
}

#[test]
fn renders_cast_and_json_variant_helpers() {
    use self::events::dsl::*;

    let cast_query = events.select((
        to_bool(success),
        to_int32(payload),
        to_int32_or_null(payload),
        to_int64_or_zero(payload),
        to_int128(id),
        to_uint32(id),
        to_uint64_or_null(payload),
        to_uint64_or_zero(payload),
        to_uint128(id),
        to_float32(id),
        to_float64_or_null(payload),
        cast::<diesel_clickhouse::sql_types::UInt64, _>(payload, "UInt64"),
        accurate_cast::<diesel::sql_types::Integer, _>(payload, "Int32"),
        accurate_cast_or_null::<diesel_clickhouse::sql_types::UInt8, _>(payload, "UInt8"),
        accurate_cast_or_default::<diesel::sql_types::Text, _>(id, "String"),
    ));
    let json_query_render = events.select((
        json_extract_string_path(payload, ["user", "name"]),
        json_extract_int_path(payload, ["metrics", "score"]),
        json_extract_raw_path(payload, ["items"]),
        json_extract_string_ci(payload, "Country"),
        json_extract_int_ci(payload, "Score"),
        json_extract_int_ci_path(payload, ["DATA", "Count"]),
        json_extract_raw_ci(payload, "Items"),
        json_has(payload, "score"),
        json_length(payload),
        json_value(payload, "$.score"),
        json_query(payload, "$.items"),
        json_exists(payload, "$.score"),
        is_valid_json(payload),
        simple_json_extract_string(payload, "country"),
        simple_json_extract_int(payload, "score"),
        simple_json_has(payload, "score"),
    ));

    assert_eq!(
        to_sql(&cast_query).unwrap(),
        "SELECT toBool(`events`.`success`), toInt32(`events`.`payload`), toInt32OrNull(`events`.`payload`), toInt64OrZero(`events`.`payload`), toInt128(`events`.`id`), toUInt32(`events`.`id`), toUInt64OrNull(`events`.`payload`), toUInt64OrZero(`events`.`payload`), toUInt128(`events`.`id`), toFloat32(`events`.`id`), toFloat64OrNull(`events`.`payload`), CAST(`events`.`payload`, 'UInt64'), accurateCast(`events`.`payload`, 'Int32'), accurateCastOrNull(`events`.`payload`, 'UInt8'), accurateCastOrDefault(`events`.`id`, 'String') FROM `events`"
    );
    assert_eq!(
        to_sql(&json_query_render).unwrap(),
        "SELECT JSONExtractString(`events`.`payload`, 'user', 'name'), JSONExtractInt(`events`.`payload`, 'metrics', 'score'), JSONExtractRaw(`events`.`payload`, 'items'), JSONExtractStringCaseInsensitive(`events`.`payload`, ?), JSONExtractIntCaseInsensitive(`events`.`payload`, ?), JSONExtractIntCaseInsensitive(`events`.`payload`, 'DATA', 'Count'), JSONExtractRawCaseInsensitive(`events`.`payload`, ?), JSONHas(`events`.`payload`, ?), JSONLength(`events`.`payload`), JSON_VALUE(`events`.`payload`, ?), JSON_QUERY(`events`.`payload`, ?), JSON_EXISTS(`events`.`payload`, ?), isValidJSON(`events`.`payload`), simpleJSONExtractString(`events`.`payload`, ?), simpleJSONExtractInt(`events`.`payload`, ?), simpleJSONHas(`events`.`payload`, ?) FROM `events`"
    );
}

#[test]
fn renders_url_ip_encoding_and_hash_functions() {
    use self::events::dsl::*;

    let url_query = events.select((
        domain(payload),
        domain_without_www(payload),
        top_level_domain(payload),
        first_significant_subdomain(payload),
        url_path(payload),
        url_path_full(payload),
        url_query_string(payload),
        url_fragment(payload),
        url_protocol(payload),
        cut_query_string(payload),
    ));
    let encoding_hash_query = events.select((
        hex(payload),
        unhex(payload),
        base64_encode(payload),
        base64_decode(payload),
        try_base64_decode(payload),
        city_hash64(payload),
        sip_hash64(payload),
        xx_hash64(payload),
        farm_fingerprint64(payload),
    ));
    let ip_query = events.select((
        to_string(to_ipv4(payload)),
        to_string(to_ipv6(payload)),
        ipv4_num_to_string(ipv4_string_to_num(payload)),
        ipv6_num_to_string(to_ipv6(payload)),
        is_ipv4_string(payload),
        is_ipv6_string(payload),
    ));

    assert_eq!(
        to_sql(&url_query).unwrap(),
        "SELECT domain(`events`.`payload`), domainWithoutWWW(`events`.`payload`), topLevelDomain(`events`.`payload`), firstSignificantSubdomain(`events`.`payload`), path(`events`.`payload`), pathFull(`events`.`payload`), queryString(`events`.`payload`), fragment(`events`.`payload`), protocol(`events`.`payload`), cutQueryString(`events`.`payload`) FROM `events`"
    );
    assert_eq!(
        to_sql(&encoding_hash_query).unwrap(),
        "SELECT hex(`events`.`payload`), unhex(`events`.`payload`), base64Encode(`events`.`payload`), base64Decode(`events`.`payload`), tryBase64Decode(`events`.`payload`), cityHash64(`events`.`payload`), sipHash64(`events`.`payload`), xxHash64(`events`.`payload`), farmFingerprint64(`events`.`payload`) FROM `events`"
    );
    assert_eq!(
        to_sql(&ip_query).unwrap(),
        "SELECT toString(toIPv4(`events`.`payload`)), toString(toIPv6(`events`.`payload`)), IPv4NumToString(IPv4StringToNum(`events`.`payload`)), IPv6NumToString(toIPv6(`events`.`payload`)), isIPv4String(`events`.`payload`), isIPv6String(`events`.`payload`) FROM `events`"
    );
}

#[test]
fn renders_higher_order_array_and_map_functions() {
    use self::events::dsl::*;

    let array_query = events.select((
        array_map(lambda("tag", "lower(tag)"), tags),
        array_filter(lambda("tag", "tag != ''"), tags),
        array_exists(lambda("tag", "tag = 'paid'"), tags),
        array_all(lambda("tag", "notEmpty(tag)"), tags),
        array_count(lambda("tag", "tag = 'paid'"), tags),
        array_exists2(lambda2("tag", "key", "tag = key"), tags, map_keys(attrs)),
    ));
    let map_query = events.select((
        map_filter(lambda2("k", "v", "k = 'country'"), attrs),
        map_apply(lambda2("k", "v", "(k, upper(v))"), attrs),
        map_from_arrays(map_keys(attrs), map_values(attrs)),
    ));

    assert_eq!(
        to_sql(&array_query).unwrap(),
        "SELECT arrayMap(tag -> lower(tag), `events`.`tags`), arrayFilter(tag -> tag != '', `events`.`tags`), arrayExists(tag -> tag = 'paid', `events`.`tags`), arrayAll(tag -> notEmpty(tag), `events`.`tags`), arrayCount(tag -> tag = 'paid', `events`.`tags`), arrayExists((tag, key) -> tag = key, `events`.`tags`, mapKeys(`events`.`attrs`)) FROM `events`"
    );
    assert_eq!(
        to_sql(&map_query).unwrap(),
        "SELECT mapFilter((k, v) -> k = 'country', `events`.`attrs`), mapApply((k, v) -> (k, upper(v)), `events`.`attrs`), mapFromArrays(mapKeys(`events`.`attrs`), mapValues(`events`.`attrs`)) FROM `events`"
    );
}

#[test]
fn array_exists2_requires_two_parameter_lambda() {
    use self::events::dsl::*;

    let query = events.select(array_exists2(
        lambda("tag", "tag = 'paid'"),
        tags,
        map_keys(attrs),
    ));
    let err = to_sql(&query).unwrap_err();
    assert!(
        err.to_string().contains("two-parameter lambda"),
        "unexpected error: {err}"
    );
}

#[test]
fn renders_parametric_aggregates() {
    use self::events::dsl::*;

    let query = events.select((
        quantile_exact(0.5, latency_ms),
        quantile_timing(0.95, latency_ms),
        quantile_deterministic(0.5, latency_ms, id),
        quantiles([0.25, 0.75], latency_ms),
        quantiles_timing([0.5, 0.95], latency_ms),
        histogram(5, latency_ms),
        top_k(3, tenant_id),
    ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT quantileExact(0.5)(`events`.`latency_ms`), quantileTiming(0.95)(`events`.`latency_ms`), quantileDeterministic(0.5)(`events`.`latency_ms`, `events`.`id`), quantiles(0.25, 0.75)(`events`.`latency_ms`), quantilesTiming(0.5, 0.95)(`events`.`latency_ms`), histogram(5)(`events`.`latency_ms`), topK(3)(`events`.`tenant_id`) FROM `events`"
    );
}

#[test]
fn renders_statistical_aggregate_functions() {
    use self::events::dsl::*;

    let query = events.select((
        corr(latency_ms, id),
        covar_pop(latency_ms, id),
        covar_samp(latency_ms, id),
        covar_pop_stable(latency_ms, id),
        covar_samp_stable(latency_ms, id),
        stddev_pop(latency_ms),
        stddev_samp(latency_ms),
        stddev_pop_stable(latency_ms),
        stddev_samp_stable(latency_ms),
        var_pop(latency_ms),
        var_samp(latency_ms),
        var_pop_stable(latency_ms),
        var_samp_stable(latency_ms),
    ));
    let test_query = events.select((
        analysis_of_variance(latency_ms, success),
        mann_whitney_u_test(latency_ms, success),
        approx_top_sum(3, tenant_id, id),
        approx_top_sum_with_reserved(3, 16, tenant_id, id),
    ));

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT corr(`events`.`latency_ms`, `events`.`id`), covarPop(`events`.`latency_ms`, `events`.`id`), covarSamp(`events`.`latency_ms`, `events`.`id`), covarPopStable(`events`.`latency_ms`, `events`.`id`), covarSampStable(`events`.`latency_ms`, `events`.`id`), stddevPop(`events`.`latency_ms`), stddevSamp(`events`.`latency_ms`), stddevPopStable(`events`.`latency_ms`), stddevSampStable(`events`.`latency_ms`), varPop(`events`.`latency_ms`), varSamp(`events`.`latency_ms`), varPopStable(`events`.`latency_ms`), varSampStable(`events`.`latency_ms`) FROM `events`"
    );
    assert_eq!(
        to_sql(&test_query).unwrap(),
        "SELECT analysisOfVariance(`events`.`latency_ms`, `events`.`success`), mannWhitneyUTest(`events`.`latency_ms`, `events`.`success`), approx_top_sum(3)(`events`.`tenant_id`, `events`.`id`), approx_top_sum(3, 16)(`events`.`tenant_id`, `events`.`id`) FROM `events`"
    );
}

#[test]
fn renders_vector_search_helpers_and_index_ddl() {
    use self::image_vectors::dsl::*;

    let reference = vector_f32([1.0, 0.0, 0.0]);
    let distance_query = image_vectors
        .select((
            id,
            l2_distance(embedding, reference.clone()),
            cosine_distance(embedding, reference.clone()),
            l1_distance(embedding, reference.clone()),
            linf_distance(embedding, reference.clone()),
            l2_norm(embedding),
            l1_norm(embedding),
            linf_norm(embedding),
        ))
        .order(cosine_distance(embedding, reference).asc())
        .limit(10);
    let literal_query = diesel::select(l2_distance(vector_f64([1.0, 2.0]), vector_f64([2.0, 4.0])));
    let binary_query = image_vectors.select((
        l2_distance(
            embedding,
            vector_f32_hex(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
        ),
        l2_distance(
            vector_f64_hex(diesel::dsl::sql::<diesel::sql_types::Text>("?")),
            vector_f64([1.0, 2.0]),
        ),
        l2_distance(
            vector_f32_binary(diesel::dsl::sql::<diesel::sql_types::Binary>("?")),
            vector_f32([1.0, 0.0, 0.0]),
        ),
    ));
    let ddl = create_table("image_vectors")
        .column("id", DataType::UInt64)
        .column("caption", DataType::String)
        .column_def(Column::new("embedding", DataType::array(DataType::Float32)).codec("NONE"))
        .index(
            vector_similarity_index("embedding_idx", "embedding", 3)
                .distance(VectorDistanceFunction::CosineDistance)
                .quantization(VectorQuantization::F32)
                .hnsw_params(32, 128)
                .granularity(100),
        )
        .engine(replacing_merge_tree().order_by(["id"]));

    assert_eq!(
        to_sql(&distance_query).unwrap(),
        "SELECT `image_vectors`.`id`, L2Distance(`image_vectors`.`embedding`, [1, 0, 0]), cosineDistance(`image_vectors`.`embedding`, [1, 0, 0]), L1Distance(`image_vectors`.`embedding`, [1, 0, 0]), LinfDistance(`image_vectors`.`embedding`, [1, 0, 0]), L2Norm(`image_vectors`.`embedding`), L1Norm(`image_vectors`.`embedding`), LinfNorm(`image_vectors`.`embedding`) FROM `image_vectors` ORDER BY cosineDistance(`image_vectors`.`embedding`, [1, 0, 0]) ASC LIMIT ?"
    );
    assert_eq!(
        to_sql(&literal_query).unwrap(),
        "SELECT L2Distance([1, 2], [2, 4])"
    );
    assert_eq!(
        to_sql(&binary_query).unwrap(),
        "SELECT L2Distance(`image_vectors`.`embedding`, reinterpret(unhex(?), 'Array(Float32)')), L2Distance(reinterpret(unhex(?), 'Array(Float64)'), [1, 2]), L2Distance(reinterpret(?, 'Array(Float32)'), [1, 0, 0]) FROM `image_vectors`"
    );
    assert_eq!(
        vector_f32_le_hex([1.0, 0.0, 0.0]),
        "0000803f0000000000000000"
    );
    assert_eq!(
        to_sql(&ddl).unwrap(),
        "CREATE TABLE `image_vectors` (\n    `id` UInt64,\n    `caption` String,\n    `embedding` Array(Float32) CODEC(NONE),\n    INDEX `embedding_idx` embedding TYPE vector_similarity('hnsw', 'cosineDistance', 3, 'f32', 32, 128) GRANULARITY 100\n) ENGINE = ReplacingMergeTree ORDER BY id"
    );
}

#[test]
fn renders_aggregate_state_and_merge_combinators() {
    use self::events::dsl::*;

    let state_query = events.group_by(tenant_id).select((
        sum_state(latency_ms),
        avg_state(latency_ms),
        min_state(id),
        max_state(id),
        count_state(),
        uniq_state(tenant_id),
        uniq_exact_state(id),
        group_array_state(id),
    ));
    let merge_query = diesel::select((
        sum_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::Double>,
        >("latency_sum")),
        avg_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::Double>,
        >("latency_avg")),
        min_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::BigInt>,
        >("min_id")),
        max_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::BigInt>,
        >("max_id")),
        count_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::BigInt>,
        >("event_count")),
        uniq_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::BigInt>,
        >("tenants")),
        uniq_exact_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::BigInt>,
        >("ids")),
        group_array_merge(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<
                diesel_clickhouse::sql_types::Array<diesel::sql_types::BigInt>,
            >,
        >("ids_array")),
        finalize_aggregation(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::Double>,
        >("latency_sum")),
    ));
    let merge_state_query = diesel::select((
        sum_merge_state(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::Double>,
        >("latency_sum")),
        avg_merge_state(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::Double>,
        >("latency_avg")),
        count_merge_state(diesel::dsl::sql::<
            diesel_clickhouse::sql_types::AggregateFunction<diesel::sql_types::BigInt>,
        >("event_count")),
    ));
    let generic_query = events.group_by(tenant_id).select((
        aggregate::<diesel::sql_types::Double>("avg")
            .arg(latency_ms)
            .or_null()
            .if_(success),
        aggregate::<diesel::sql_types::Double>("sum")
            .arg(latency_ms)
            .if_(success),
        aggregate::<diesel::sql_types::BigInt>("count")
            .no_args()
            .if_(success),
        aggregate::<diesel::sql_types::Double>("avg")
            .arg(latency_ms)
            .state(),
        aggregate::<diesel::sql_types::BigInt>("uniq")
            .arg(tenant_id)
            .distinct(),
    ));

    assert_eq!(
        to_sql(&state_query).unwrap(),
        "SELECT sumState(`events`.`latency_ms`), avgState(`events`.`latency_ms`), minState(`events`.`id`), maxState(`events`.`id`), countState(), uniqState(`events`.`tenant_id`), uniqExactState(`events`.`id`), groupArrayState(`events`.`id`) FROM `events` GROUP BY `events`.`tenant_id`"
    );
    assert_eq!(
        to_sql(&merge_query).unwrap(),
        "SELECT sumMerge(latency_sum), avgMerge(latency_avg), minMerge(min_id), maxMerge(max_id), countMerge(event_count), uniqMerge(tenants), uniqExactMerge(ids), groupArrayMerge(ids_array), finalizeAggregation(latency_sum)"
    );
    assert_eq!(
        to_sql(&merge_state_query).unwrap(),
        "SELECT sumMergeState(latency_sum), avgMergeState(latency_avg), countMergeState(event_count)"
    );
    assert_eq!(
        to_sql(&generic_query).unwrap(),
        "SELECT avgOrNullIf(`events`.`latency_ms`, `events`.`success`), sumIf(`events`.`latency_ms`, `events`.`success`), countIf(`events`.`success`), avgState(`events`.`latency_ms`), uniqDistinct(`events`.`tenant_id`) FROM `events` GROUP BY `events`.`tenant_id`"
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
            .over_ch(partition_by(tenant_id).order_by(id.desc()))
            .eq(1_i64),
    );
    let named_window_query = events
        .select((
            tenant_id,
            rank().over_window("by_tenant"),
            dense_rank().over_ch(partition_by(tenant_id).order_by(latency_ms.desc())),
            lag_in_frame(latency_ms, 1_i64, 0.0).over_ch(
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
fn renders_window_frame_variants() {
    use self::events::dsl::*;

    let rolling_rows = events.select((
        id,
        diesel::dsl::sql::<diesel::sql_types::Double>("sum(`events`.`latency_ms`)").over_ch(
            partition_by(tenant_id)
                .order_by(id.asc())
                .rows_between_preceding_and_following(1, 1),
        ),
    ));
    let trailing_range = events.select((
        id,
        diesel::dsl::sql::<diesel::sql_types::Double>("avg(`events`.`latency_ms`)").over_ch(
            partition_by(tenant_id)
                .order_by(latency_ms)
                .range_between_unbounded_preceding_and_current_row(),
        ),
    ));
    let generic_rows = events.select((
        id,
        diesel::dsl::sql::<diesel::sql_types::Double>("max(`events`.`latency_ms`)").over_ch(
            partition_by(tenant_id)
                .order_by(id)
                .rows_between(WindowFrameBound::CurrentRow, WindowFrameBound::following(2)),
        ),
    ));

    assert_eq!(
        to_sql(&rolling_rows).unwrap(),
        "SELECT `events`.`id`, sum(`events`.`latency_ms`) OVER (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`id` ASC ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM `events`"
    );
    assert_eq!(
        to_sql(&trailing_range).unwrap(),
        "SELECT `events`.`id`, avg(`events`.`latency_ms`) OVER (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`latency_ms` RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM `events`"
    );
    assert_eq!(
        to_sql(&generic_rows).unwrap(),
        "SELECT `events`.`id`, max(`events`.`latency_ms`) OVER (PARTITION BY `events`.`tenant_id` ORDER BY `events`.`id` ROWS BETWEEN CURRENT ROW AND 2 FOLLOWING) FROM `events`"
    );
}

#[test]
fn renders_clickhouse_join_extensions() {
    use self::events::dsl::*;

    let any_join = events
        .clickhouse_join(tenants::table)
        .any()
        .inner()
        .on(tenant_id.eq(tenants::tenant_id))
        .select(diesel::dsl::sql::<(
            diesel::sql_types::Text,
            diesel::sql_types::Text,
        )>("`events`.`tenant_id`, `tenants`.`plan`"));
    let using_join = events
        .clickhouse_join(tenants::table)
        .global()
        .all()
        .left()
        .outer()
        .using(["tenant_id"])
        .select(diesel::dsl::sql::<(
            diesel::sql_types::Text,
            diesel::sql_types::Text,
        )>("`events`.`tenant_id`, `tenants`.`plan`"));
    let semi_join = events
        .clickhouse_join(tenants::table)
        .left()
        .semi()
        .using(["tenant_id"])
        .select(diesel::dsl::sql::<diesel::sql_types::Text>(
            "`events`.`tenant_id`",
        ));
    let anti_join = tenants::table
        .clickhouse_join(events)
        .left()
        .anti()
        .using(["tenant_id"])
        .select(diesel::dsl::sql::<diesel::sql_types::Text>(
            "`tenants`.`tenant_id`",
        ));
    let asof_join = events
        .clickhouse_join(tenant_rates::table)
        .asof()
        .left()
        .on(diesel::dsl::sql::<diesel::sql_types::Bool>(
            "`events`.`tenant_id` = `tenant_rates`.`tenant_id` AND `events`.`created_at` >= `tenant_rates`.`effective_at`",
        ))
        .select(diesel::dsl::sql::<(diesel::sql_types::BigInt, diesel::sql_types::Double)>(
            "`events`.`id`, `tenant_rates`.`rate`",
        ));

    assert_eq!(
        to_sql(&any_join).unwrap(),
        "SELECT `events`.`tenant_id`, `tenants`.`plan` FROM `events` ANY INNER JOIN `tenants` ON (`events`.`tenant_id` = `tenants`.`tenant_id`)"
    );
    assert_eq!(
        to_sql(&using_join).unwrap(),
        "SELECT `events`.`tenant_id`, `tenants`.`plan` FROM `events` GLOBAL ALL LEFT OUTER JOIN `tenants` USING (`tenant_id`)"
    );
    assert_eq!(
        to_sql(&semi_join).unwrap(),
        "SELECT `events`.`tenant_id` FROM `events` LEFT SEMI JOIN `tenants` USING (`tenant_id`)"
    );
    assert_eq!(
        to_sql(&anti_join).unwrap(),
        "SELECT `tenants`.`tenant_id` FROM `tenants` LEFT ANTI JOIN `events` USING (`tenant_id`)"
    );
    assert_eq!(
        to_sql(&asof_join).unwrap(),
        "SELECT `events`.`id`, `tenant_rates`.`rate` FROM `events` ASOF LEFT JOIN `tenant_rates` ON `events`.`tenant_id` = `tenant_rates`.`tenant_id` AND `events`.`created_at` >= `tenant_rates`.`effective_at`"
    );
}

#[test]
fn renders_clickhouse_join_with_typed_columns() {
    use self::events::dsl::*;

    // `join_column` gives type-safe, table-qualified projection from a custom
    // ClickHouse join — no hand-written `sql::<...>("...")` select list — and
    // renders identically to the raw form in `renders_clickhouse_join_extensions`.
    let any_join = events
        .clickhouse_join(tenants::table)
        .any()
        .inner()
        .on(tenant_id.eq(tenants::tenant_id))
        .select((join_column(id), join_column(tenants::plan)));
    assert_eq!(
        to_sql(&any_join).unwrap(),
        "SELECT `events`.`id`, `tenants`.`plan` FROM `events` ANY INNER JOIN `tenants` ON (`events`.`tenant_id` = `tenants`.`tenant_id`)"
    );

    // The wrapped column keeps its SQL type, so the query's SqlType is the
    // typed tuple `(BigInt, Text)` rather than the untyped `sql(...)` shape.
    fn assert_sql_type<Q>(_: &Q)
    where
        Q: diesel::query_builder::Query<
                SqlType = (diesel::sql_types::BigInt, diesel::sql_types::Text),
            >,
    {
    }
    assert_sql_type(&any_join);
}

#[test]
fn renders_typed_predicates_on_custom_sources() {
    use self::events::dsl::*;

    // Typed `.filter(...)` predicates over a custom `ClickHouseJoin` source —
    // real, type-checked columns rather than `sql::<Bool>("...")`. Diesel does
    // not implement `FilterDsl` for the join source itself, so `.select(...)`
    // comes first; the resulting statement then filters by typed columns from
    // either side of the join.
    let filtered_join = events
        .clickhouse_join(tenants::table)
        .any()
        .inner()
        .on(tenant_id.eq(tenants::tenant_id))
        .select((source_column(id), source_column(tenants::plan)))
        .filter(tenant_id.eq("acme"))
        .filter(tenants::plan.eq("enterprise"));
    assert_eq!(
        to_sql(&filtered_join).unwrap(),
        "SELECT `events`.`id`, `tenants`.`plan` FROM `events` ANY INNER JOIN `tenants` \
         ON (`events`.`tenant_id` = `tenants`.`tenant_id`) \
         WHERE ((`events`.`tenant_id` = ?) AND (`tenants`.`plan` = ?))"
    );

    // Native `count()` renders ClickHouse's `count()` and is typed `UInt64`, so
    // it loads into a `u64` (unlike Diesel's `count_star()`, which is `BigInt`).
    let count_query = events.group_by(tenant_id).select((tenant_id, count()));
    assert_eq!(
        to_sql(&count_query).unwrap(),
        "SELECT `events`.`tenant_id`, count() FROM `events` GROUP BY `events`.`tenant_id`"
    );

    fn assert_count_is_uint64<Q>(_: &Q)
    where
        Q: diesel::query_builder::Query<
                SqlType = (
                    diesel::sql_types::Text,
                    diesel_clickhouse::sql_types::UInt64,
                ),
            >,
    {
    }
    assert_count_is_uint64(&count_query);

    // `expr_as` aliases any expression (here an aggregate), so it loads into a
    // named/struct-friendly field — the general form of `source_column_as`.
    let aliased = events
        .group_by(tenant_id)
        .select((source_column_as(tenant_id, "tenant"), expr_as(count(), "n")));
    assert_eq!(
        to_sql(&aliased).unwrap(),
        "SELECT `events`.`tenant_id` AS `tenant`, count() AS `n` FROM `events` \
         GROUP BY `events`.`tenant_id`"
    );
}

#[test]
fn renders_extended_clickhouse_data_types() {
    let ddl = create_table("analytics.type_showcase")
        .column("big_signed", DataType::Int128)
        .column("huge_signed", DataType::Int256)
        .column("big_unsigned", DataType::UInt128)
        .column("huge_unsigned", DataType::UInt256)
        .column("compact_float", DataType::BFloat16)
        .column("amount32", DataType::decimal32(2))
        .column("amount64", DataType::decimal64(4))
        .column("amount128", DataType::decimal128(8))
        .column("amount256", DataType::decimal256(12))
        .column("amount", DataType::decimal(18, 6))
        .column(
            "status",
            DataType::enum8([("draft", 1), ("published", 2), ("archived", 3)]),
        )
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

    assert_eq!(
        to_sql(&ddl).unwrap(),
        "CREATE TABLE `analytics`.`type_showcase` (\n    `big_signed` Int128,\n    `huge_signed` Int256,\n    `big_unsigned` UInt128,\n    `huge_unsigned` UInt256,\n    `compact_float` BFloat16,\n    `amount32` Decimal32(2),\n    `amount64` Decimal64(4),\n    `amount128` Decimal128(8),\n    `amount256` Decimal256(12),\n    `amount` Decimal(18, 6),\n    `status` Enum8('draft' = 1, 'published' = 2, 'archived' = 3),\n    `kind` Enum16('organic' = 100, 'paid' = 200),\n    `location` Point,\n    `boundary` Ring,\n    `flex` Dynamic(max_types=4),\n    `variant_value` Variant(UInt64, String, Array(UInt64)),\n    `dimensions` Tuple(String, UInt64, Float64),\n    `attributes` Nested(`key` String, `value` String)\n) ENGINE = Memory"
    );
}

#[test]
fn renders_create_table_mergetree_ddl() {
    let ddl = create_table("analytics.events")
        .if_not_exists()
        .column("id", DataType::UInt64)
        .column("tenant_id", DataType::low_cardinality(DataType::String))
        .column("created_at", DataType::DateTime)
        .column("success", DataType::Bool)
        .column("latency_ms", DataType::Float64)
        .column("tags", DataType::array(DataType::String))
        .column("attrs", DataType::map(DataType::String, DataType::String))
        .column("client_ip", DataType::IPv4)
        .column("client_ip6", DataType::IPv6)
        .column(
            "latency_state",
            DataType::aggregate_function("sum", [DataType::Float64]),
        )
        .column(
            "ids_state",
            DataType::aggregate_function("uniqExact", [DataType::UInt64]),
        )
        .column_def(Column::new("payload", DataType::String).codec("ZSTD(1)"))
        .column_def(Column::new("created_date", DataType::Date).alias_expr("toDate(created_at)"))
        .engine(
            replacing_merge_tree()
                .partition_by(["toDate(created_at)"])
                .primary_key(["tenant_id", "id"])
                .order_by(["tenant_id", "id"])
                .sample_by("id")
                .ttl("created_at + INTERVAL 7 DAY")
                .setting("index_granularity", 8192_i64),
        );

    assert_eq!(
        to_sql(&ddl).unwrap(),
        "CREATE TABLE IF NOT EXISTS `analytics`.`events` (\n    `id` UInt64,\n    `tenant_id` LowCardinality(String),\n    `created_at` DateTime,\n    `success` Bool,\n    `latency_ms` Float64,\n    `tags` Array(String),\n    `attrs` Map(String, String),\n    `client_ip` IPv4,\n    `client_ip6` IPv6,\n    `latency_state` AggregateFunction(sum, Float64),\n    `ids_state` AggregateFunction(uniqExact, UInt64),\n    `payload` String CODEC(ZSTD(1)),\n    `created_date` Date ALIAS toDate(created_at)\n) ENGINE = ReplacingMergeTree PARTITION BY toDate(created_at) PRIMARY KEY (tenant_id, id) ORDER BY (tenant_id, id) SAMPLE BY id TTL created_at + INTERVAL 7 DAY SETTINGS index_granularity = 8192"
    );
}

#[test]
fn renders_table_engine_and_projection_ddl_depth() {
    let rollup = create_table("analytics.rollups")
        .column("tenant_id", DataType::String)
        .column("bucket", DataType::Date)
        .column("hits", DataType::UInt64)
        .column("bytes", DataType::UInt64)
        .projection(projection(
            "by_tenant",
            "SELECT tenant_id, sum(hits), sum(bytes) GROUP BY tenant_id",
        ))
        .engine(
            summing_merge_tree_with(["hits", "bytes"])
                .partition_by(["toYYYYMM(bucket)"])
                .order_by(["tenant_id", "bucket"])
                .setting("index_granularity", 2048_i64),
        );
    let aggregate = create_table("analytics.aggregate_states")
        .column("tenant_id", DataType::String)
        .column(
            "hits",
            DataType::aggregate_function("sum", [DataType::UInt64]),
        )
        .engine(aggregating_merge_tree().order_by(["tenant_id"]));
    let collapsing = create_table("analytics.collapsing_events")
        .column("id", DataType::UInt64)
        .column("sign", DataType::Int8)
        .engine(collapsing_merge_tree("sign").order_by(["id"]));
    let versioned = create_table("analytics.versioned_events")
        .column("id", DataType::UInt64)
        .column("sign", DataType::Int8)
        .column("version", DataType::UInt64)
        .engine(versioned_collapsing_merge_tree("sign", "version").order_by(["id"]));
    let distributed_table = create_table("analytics.events_all")
        .column("id", DataType::UInt64)
        .engine(
            distributed("company_cluster", "analytics", "events_local")
                .sharding_key("cityHash64(id)")
                .policy_name("fast_distributed")
                .setting("fsync_after_insert", false),
        );
    let buffer_table = create_table("analytics.events_buffer")
        .column("id", DataType::UInt64)
        .engine(
            buffer(
                "analytics",
                "events_local",
                1,
                10,
                100,
                1_000,
                1_000_000,
                1_048_576,
                104_857_600,
            )
            .flush_time(5)
            .flush_rows(10_000)
            .flush_bytes(10_485_760),
        );
    let null_table = create_table("analytics.discarded_events")
        .column("id", DataType::UInt64)
        .engine(TableEngine::null());
    let add_projection = alter_table("analytics.rollups").add_projection(projection(
        "by_bucket",
        "SELECT bucket, sum(hits) GROUP BY bucket",
    ));
    let materialize_projection = alter_table("analytics.rollups")
        .materialize_projection("by_bucket")
        .setting("mutations_sync", 2_i64);
    let drop_projection = alter_table("analytics.rollups").drop_projection("by_bucket");

    assert_eq!(
        to_sql(&rollup).unwrap(),
        "CREATE TABLE `analytics`.`rollups` (\n    `tenant_id` String,\n    `bucket` Date,\n    `hits` UInt64,\n    `bytes` UInt64,\n    PROJECTION `by_tenant` (SELECT tenant_id, sum(hits), sum(bytes) GROUP BY tenant_id)\n) ENGINE = SummingMergeTree((hits, bytes)) PARTITION BY toYYYYMM(bucket) ORDER BY (tenant_id, bucket) SETTINGS index_granularity = 2048"
    );
    assert_eq!(
        to_sql(&aggregate).unwrap(),
        "CREATE TABLE `analytics`.`aggregate_states` (\n    `tenant_id` String,\n    `hits` AggregateFunction(sum, UInt64)\n) ENGINE = AggregatingMergeTree ORDER BY tenant_id"
    );
    assert_eq!(
        to_sql(&collapsing).unwrap(),
        "CREATE TABLE `analytics`.`collapsing_events` (\n    `id` UInt64,\n    `sign` Int8\n) ENGINE = CollapsingMergeTree(sign) ORDER BY id"
    );
    assert_eq!(
        to_sql(&versioned).unwrap(),
        "CREATE TABLE `analytics`.`versioned_events` (\n    `id` UInt64,\n    `sign` Int8,\n    `version` UInt64\n) ENGINE = VersionedCollapsingMergeTree(sign, version) ORDER BY id"
    );
    assert_eq!(
        to_sql(&distributed_table).unwrap(),
        "CREATE TABLE `analytics`.`events_all` (\n    `id` UInt64\n) ENGINE = Distributed(company_cluster, analytics, events_local, cityHash64(id), fast_distributed) SETTINGS fsync_after_insert = 0"
    );
    assert_eq!(
        to_sql(&buffer_table).unwrap(),
        "CREATE TABLE `analytics`.`events_buffer` (\n    `id` UInt64\n) ENGINE = Buffer(analytics, events_local, 1, 10, 100, 1000, 1000000, 1048576, 104857600, 5, 10000, 10485760)"
    );
    assert_eq!(
        to_sql(&null_table).unwrap(),
        "CREATE TABLE `analytics`.`discarded_events` (\n    `id` UInt64\n) ENGINE = Null"
    );
    assert_eq!(
        to_sql(&add_projection).unwrap(),
        "ALTER TABLE `analytics`.`rollups` ADD PROJECTION `by_bucket` (SELECT bucket, sum(hits) GROUP BY bucket)"
    );
    assert_eq!(
        to_sql(&materialize_projection).unwrap(),
        "ALTER TABLE `analytics`.`rollups` MATERIALIZE PROJECTION `by_bucket` SETTINGS mutations_sync = 2"
    );
    assert_eq!(
        to_sql(&drop_projection).unwrap(),
        "ALTER TABLE `analytics`.`rollups` DROP PROJECTION `by_bucket`"
    );
}

#[test]
fn renders_alter_table_helpers() {
    let add_column = alter_table("analytics.events").add_column_after(
        Column::new("page", DataType::String).default_expr("'home'"),
        "id",
    );
    let rename_column = alter_table("analytics.events").rename_column("page", "page_name");
    let drop_column = alter_table("analytics.events").drop_column("page_name");
    let add_index = alter_table("analytics.events")
        .add_index(TableIndex::custom("id_minmax", "id", "minmax").granularity(1));
    let materialize_index = alter_table("analytics.events")
        .materialize_index("id_minmax")
        .setting("mutations_sync", 2_i64);
    let drop_index = alter_table("analytics.events").drop_index("id_minmax");
    let ttl = alter_table("analytics.events").modify_ttl("created_at + INTERVAL 30 DAY");

    assert_eq!(
        to_sql(&add_column).unwrap(),
        "ALTER TABLE `analytics`.`events` ADD COLUMN `page` String DEFAULT 'home' AFTER `id`"
    );
    assert_eq!(
        to_sql(&rename_column).unwrap(),
        "ALTER TABLE `analytics`.`events` RENAME COLUMN `page` TO `page_name`"
    );
    assert_eq!(
        to_sql(&drop_column).unwrap(),
        "ALTER TABLE `analytics`.`events` DROP COLUMN `page_name`"
    );
    assert_eq!(
        to_sql(&add_index).unwrap(),
        "ALTER TABLE `analytics`.`events` ADD INDEX `id_minmax` id TYPE minmax GRANULARITY 1"
    );
    assert_eq!(
        to_sql(&materialize_index).unwrap(),
        "ALTER TABLE `analytics`.`events` MATERIALIZE INDEX `id_minmax` SETTINGS mutations_sync = 2"
    );
    assert_eq!(
        to_sql(&drop_index).unwrap(),
        "ALTER TABLE `analytics`.`events` DROP INDEX `id_minmax`"
    );
    assert_eq!(
        to_sql(&ttl).unwrap(),
        "ALTER TABLE `analytics`.`events` MODIFY TTL created_at + INTERVAL 30 DAY"
    );
}

#[test]
fn renders_alter_table_mutation_and_partition_helpers() {
    let update = alter_table("analytics.events")
        .update_in_partition(
            [
                mutation_assignment("page", "'landing'"),
                mutation_assignment("latency_ms", "latency_ms + 1"),
            ],
            partition_expr("'2024-01-01'"),
            "tenant_id = 'acme'",
        )
        .setting("mutations_sync", 2_i64);
    let delete = alter_table("analytics.events")
        .delete_in_partition(partition_id("202401"), "success = 0")
        .setting("mutations_sync", 2_i64);
    let drop_partition = alter_table("analytics.events").drop_partition(partition_id("202401"));
    let detach_partition =
        alter_table("analytics.events").detach_partition(partition_expr("tuple(2024, 1)"));
    let attach_partition = alter_table("analytics.events").attach_partition(partition_expr("ALL"));
    let drop_detached =
        alter_table("analytics.events").drop_detached_partition(partition_id("old"));
    let freeze = alter_table("analytics.events")
        .freeze_partition_with_name(partition_expr("'2024-01-01'"), "backup_2024_01_01");
    let freeze_all = alter_table("analytics.events").freeze_with_name("backup_all");
    let clear_column =
        alter_table("analytics.events").clear_column_in_partition("page", partition_id("202401"));
    let clear_index = alter_table("analytics.events")
        .clear_index_in_partition("id_minmax", partition_expr("'2024-01-01'"));
    let replace_partition = alter_table("analytics.events")
        .replace_partition_from(partition_expr("'2024-01-01'"), "analytics.events_staging");
    let move_partition = alter_table("analytics.events")
        .move_partition_to_table(partition_expr("'2024-01-01'"), "analytics.events_archive");

    assert_eq!(
        to_sql(&update).unwrap(),
        "ALTER TABLE `analytics`.`events` UPDATE `page` = 'landing', `latency_ms` = latency_ms + 1 IN PARTITION '2024-01-01' WHERE tenant_id = 'acme' SETTINGS mutations_sync = 2"
    );
    assert_eq!(
        to_sql(&delete).unwrap(),
        "ALTER TABLE `analytics`.`events` DELETE IN PARTITION ID '202401' WHERE success = 0 SETTINGS mutations_sync = 2"
    );
    assert_eq!(
        to_sql(&drop_partition).unwrap(),
        "ALTER TABLE `analytics`.`events` DROP PARTITION ID '202401'"
    );
    assert_eq!(
        to_sql(&detach_partition).unwrap(),
        "ALTER TABLE `analytics`.`events` DETACH PARTITION tuple(2024, 1)"
    );
    assert_eq!(
        to_sql(&attach_partition).unwrap(),
        "ALTER TABLE `analytics`.`events` ATTACH PARTITION ALL"
    );
    assert_eq!(
        to_sql(&drop_detached).unwrap(),
        "ALTER TABLE `analytics`.`events` DROP DETACHED PARTITION ID 'old'"
    );
    assert_eq!(
        to_sql(&freeze).unwrap(),
        "ALTER TABLE `analytics`.`events` FREEZE PARTITION '2024-01-01' WITH NAME 'backup_2024_01_01'"
    );
    assert_eq!(
        to_sql(&freeze_all).unwrap(),
        "ALTER TABLE `analytics`.`events` FREEZE WITH NAME 'backup_all'"
    );
    assert_eq!(
        to_sql(&clear_column).unwrap(),
        "ALTER TABLE `analytics`.`events` CLEAR COLUMN `page` IN PARTITION ID '202401'"
    );
    assert_eq!(
        to_sql(&clear_index).unwrap(),
        "ALTER TABLE `analytics`.`events` CLEAR INDEX `id_minmax` IN PARTITION '2024-01-01'"
    );
    assert_eq!(
        to_sql(&replace_partition).unwrap(),
        "ALTER TABLE `analytics`.`events` REPLACE PARTITION '2024-01-01' FROM `analytics`.`events_staging`"
    );
    assert_eq!(
        to_sql(&move_partition).unwrap(),
        "ALTER TABLE `analytics`.`events` MOVE PARTITION '2024-01-01' TO TABLE `analytics`.`events_archive`"
    );
}

#[test]
fn renders_create_materialized_view_ddl() {
    use self::events::dsl::*;

    let to_view = create_materialized_view("analytics.events_by_tenant_mv")
        .if_not_exists()
        .to("analytics.events_by_tenant")
        .as_select(
            events
                .group_by(tenant_id)
                .select((tenant_id, count_if(success))),
        );
    let engine_view = create_materialized_view("analytics.events_owned_mv")
        .engine(TableEngine::custom("SummingMergeTree ORDER BY tenant_id"))
        .populate()
        .as_select(
            events
                .group_by(tenant_id)
                .select((tenant_id, count_if(success))),
        );

    assert_eq!(
        to_sql(&to_view).unwrap(),
        "CREATE MATERIALIZED VIEW IF NOT EXISTS `analytics`.`events_by_tenant_mv` TO `analytics`.`events_by_tenant` AS SELECT `events`.`tenant_id`, countIf(`events`.`success`) FROM `events` GROUP BY `events`.`tenant_id`"
    );
    assert_eq!(
        to_sql(&engine_view).unwrap(),
        "CREATE MATERIALIZED VIEW `analytics`.`events_owned_mv` ENGINE = SummingMergeTree ORDER BY tenant_id POPULATE AS SELECT `events`.`tenant_id`, countIf(`events`.`success`) FROM `events` GROUP BY `events`.`tenant_id`"
    );
}

#[test]
fn renders_into_outfile_query_wrapper() {
    use self::events::dsl::*;

    let csv_export = events
        .select((id, tenant_id))
        .filter(success.eq(true))
        .into_outfile("exports/events.csv.gz")
        .and_stdout()
        .truncate()
        .compression_with_level(OutfileCompression::Gzip, 6)
        .format(Format::Csv);
    let append_export = events
        .select(id)
        .into_outfile("exports/events.tsv")
        .append();
    let no_compression_export = events
        .select(id)
        .into_outfile("exports/events.raw")
        .compression(OutfileCompression::None);

    assert_eq!(
        to_sql(&csv_export).unwrap(),
        "SELECT `events`.`id`, `events`.`tenant_id` FROM `events` WHERE (`events`.`success` = ?) INTO OUTFILE 'exports/events.csv.gz' AND STDOUT TRUNCATE COMPRESSION 'gzip' LEVEL 6 FORMAT CSV"
    );
    assert_eq!(
        to_sql(&append_export).unwrap(),
        "SELECT `events`.`id` FROM `events` INTO OUTFILE 'exports/events.tsv' APPEND"
    );
    assert_eq!(
        to_sql(&no_compression_export).unwrap(),
        "SELECT `events`.`id` FROM `events` INTO OUTFILE 'exports/events.raw' COMPRESSION 'none'"
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

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    counters (id) {
        id -> UInt64,
        shard -> UInt32,
    }
}

// `bind` lets a Rust `u64`/`u32` be compared against a ClickHouse unsigned column
// without the untyped `sql::<UInt64>("?")` escape hatch. The target SQL type is
// inferred from the column, so no turbofish is needed, and each value renders as
// a real `?` bind parameter.
#[test]
fn renders_bind_against_clickhouse_unsigned_columns() {
    use diesel_clickhouse::bind;

    let after_id: u64 = 42;
    let shard: u32 = 7;
    let query = counters::table
        .filter(counters::id.gt(bind(after_id)))
        .filter(counters::shard.eq(bind(shard)))
        .select(counters::id);

    assert_eq!(
        to_sql(&query).unwrap(),
        "SELECT `counters`.`id` FROM `counters` WHERE ((`counters`.`id` > ?) AND (`counters`.`shard` = ?))"
    );
}

// `when(enabled, predicate)` renders the predicate when enabled and the
// always-true constant `1` (binding nothing) when disabled, giving an optional
// filter on a backend that does not support boxed queries.
#[test]
fn renders_when_optional_filter() {
    use self::events::dsl::*;
    use diesel_clickhouse::when;

    let enabled = events.filter(when(true, tenant_id.eq("acme"))).select(id);
    assert_eq!(
        to_sql(&enabled).unwrap(),
        "SELECT `events`.`id` FROM `events` WHERE (`events`.`tenant_id` = ?)"
    );

    let disabled = events.filter(when(false, tenant_id.eq("acme"))).select(id);
    assert_eq!(
        to_sql(&disabled).unwrap(),
        "SELECT `events`.`id` FROM `events` WHERE 1"
    );
}
