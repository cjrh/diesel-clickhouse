//! Diesel query-builder extensions for ClickHouse.
//!
//! This crate follows the same shape as small Diesel extension crates such as
//! `diesel-paradedb`: it adds typed AST nodes for SQL that Diesel does not know
//! about natively.  The [`ClickHouse`] backend is a lightweight SQL-rendering
//! backend, so regular Diesel expressions can be rendered with ClickHouse-style
//! identifiers and placeholders while ClickHouse-only functions, operators and
//! trailing clauses compose with the usual `select` / `filter` / `order` calls.
//!
//! ## Quick start
//!
//! ```ignore
//! use diesel::prelude::*;
//! use diesel_clickhouse::{
//!     count_if, format, limit_by_col, quantile, to_start_of_hour, ClickHouse,
//!     ClickHouseQueryDsl, Format,
//! };
//!
//! let query = events::table
//!     .select((
//!         to_start_of_hour(events::created_at),
//!         count_if(events::success.eq(true)),
//!         quantile(0.95, events::latency_ms),
//!     ))
//!     .filter(events::tenant_id.eq("acme"))
//!     .group_by(to_start_of_hour(events::created_at))
//!     .order(to_start_of_hour(events::created_at).desc())
//!     .limit_by_col(10, "tenant_id")
//!     .format(Format::JsonEachRow);
//!
//! let sql = diesel_clickhouse::to_sql(&query)?;
//! # Ok::<(), diesel::result::Error>(())
//! ```
//!
//! ## Scope
//!
//! This crate focuses on **building ClickHouse SQL from Diesel ASTs** and also
//! includes a native async, HTTP-backed [`AsyncClickHouseConnection`] (a
//! [`diesel_async::AsyncConnection`]) plus explicit
//! [`ClickHouseConnectionOptions`] for idiomatic async Diesel
//! `load`/`execute`/`batch_execute` workflows. You can still execute rendered
//! SQL through your ClickHouse client of choice when you need client-specific
//! behavior.
//!
//! ## Start here: choose your path
//!
//! - **Want Diesel to own the bind values?** Execute with
//!   [`AsyncClickHouseConnection`] and `.load`/`.execute`.
//! - **Rendering SQL for another ClickHouse client?** Use
//!   [`to_sql_with_metadata`] and re-supply the bind values yourself.
//! - **Need bulk ingestion?** Use [`AsyncClickHouseConnection::insert_batch`].
//! - **Need ClickHouse syntax with no typed binding?** Use
//!   `diesel::dsl::sql::<T>(...)`, but know its contents are unchecked.
//!
//! ## Guides
//!
//! Long-form Markdown guides from `./docs/` are rendered under [`docs`] on
//! docs.rs:
//!
//! - [`docs::usage`] for the model: execution modes, the safety trade-offs, caveats.
//! - [`docs::cookbook`] for copyable "how do I write this query?" recipes, each
//!   verified by running its raw SQL and Diesel form and asserting equal results.
//! - [`docs::tutorial`] for the NYC taxi tutorial translated to Diesel.
//! - [`docs::feature_matrix`] for the implementation checklist.
//! - [`docs::connection_design`] for connection design notes.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/diesel-clickhouse/0.1.0")]

mod aggregates;
mod backend;
mod cast;
mod clauses;
mod connection;
mod ddl;
pub mod docs;
mod functions;
mod grouping;
mod higher_order;
mod joins;
mod json;
mod operators;
mod ordering;
mod serialize;
mod types;
mod vectors;
mod window;

pub use aggregates::{
    AggregateBuilder, AggregateCall, AggregateIfArgs, ApproxTopSumItem, ApproxTopSumResult,
    BinaryParametricAggregate, Histogram, HistogramBucket, NoAggregateArgs, OneAggregateArg,
    ParametricAggregate, Quantile, TwoAggregateArgs, aggregate, approx_top_sum,
    approx_top_sum_with_reserved, histogram, quantile, quantile_deterministic, quantile_exact,
    quantile_tdigest, quantile_timing, quantiles, quantiles_timing, top_k,
};
pub use backend::{
    ClickHouse, ClickHouseQueryBuilder, ClickHouseTypeMetadata, RenderedSql, RenderedSqlMetadata,
    analyze_rendered_sql, to_sql, to_sql_with_metadata,
};
pub use cast::{
    CastFunction, accurate_cast, accurate_cast_or_default, accurate_cast_or_null, cast,
};
pub use clauses::{
    AliasedSource, ArrayJoin, ArrayJoinKind, ClickHouseQueryDsl, Final, Format, FormattedQuery,
    IntoOutfileQuery, LimitBy, LimitWithTies, NoSampleOffset, NoWithBindings, OutfileCompression,
    OutfileMode, Prewhere, Sample, SampleOffset, Setting, SettingValue, SettingsQuery, WithBinding,
    WithCteBinding, WithQuery, alias_source, array_join_clause, array_join_clause_as, final_table,
    format, into_outfile, left_array_join_clause, left_array_join_clause_as, limit_by_col,
    prewhere, sample, sample_offset, settings, with_alias, with_cte, with_materialized_cte,
    with_ties,
};
pub use clickhouse;
pub use connection::{
    AsyncClickHouseConnection, ClickHouseConnectionOptions, ClickHouseField, ClickHouseRow,
    ClickHouseTransactionManager,
};
pub use ddl::{
    AlterTable, BufferEngine, Column, CreateMaterializedView, CreateMaterializedViewBuilder,
    CreateTable, DataType, DistributedEngine, EngineSetting, EngineSettingValue, IndexType,
    MergeTree, MutationAssignment, NestedField, PartitionExpr, TableEngine, TableIndex,
    TableProjection, VectorDistanceFunction, VectorIndexAlgorithm, VectorQuantization,
    VectorSimilarityIndex, aggregating_merge_tree, all_partitions, alter_table, buffer,
    collapsing_merge_tree, create_materialized_view, create_table, distributed, merge_tree,
    mutation_assignment, partition_expr, partition_id, projection, replacing_merge_tree,
    replacing_merge_tree_with, summing_merge_tree, summing_merge_tree_with,
    vector_similarity_index, versioned_collapsing_merge_tree,
};
pub use functions::{
    abs, analysis_of_variance, any_last, any_value, arg_max, arg_min, array_concat, array_distinct,
    array_element, array_join, avg_if, avg_merge, avg_merge_state, avg_state, base64_decode,
    base64_encode, ceil, city_hash64, concat, corr, cosine_distance, count, count_if, count_merge,
    count_merge_state, count_state, covar_pop, covar_pop_stable, covar_samp, covar_samp_stable,
    cut_query_string, date_diff, date_trunc, domain, domain_without_www, empty, farm_fingerprint64,
    finalize_aggregation, first_significant_subdomain, floor, greatest, group_array,
    group_array_if, group_array_merge, group_array_state, has, has_all, has_any, hex, if_, ilike,
    ilike_escape, int_div, ipv4_num_to_string, ipv4_string_to_num, ipv6_num_to_string,
    is_ipv4_string, is_ipv6_string, is_not_null, is_null, is_valid_json, json_exists,
    json_extract_bool, json_extract_float, json_extract_int, json_extract_int_ci, json_extract_raw,
    json_extract_raw_ci, json_extract_string, json_extract_string_ci, json_extract_uint, json_has,
    json_length, json_query, json_value, l1_distance, l1_norm, l2_distance, l2_norm, least, length,
    like, like_escape, linf_distance, linf_norm, lower, mann_whitney_u_test, map_contains,
    map_from_arrays, map_keys, map_values, max_if, max_merge, max_state, min_if, min_merge,
    min_state, multi_fuzzy_match_all_indices, multi_fuzzy_match_any, multi_fuzzy_match_any_index,
    multi_match_all_indices, multi_match_any, multi_match_any_index, not_empty, not_ilike,
    not_ilike_escape, not_like, not_like_escape, position, regexp_match, replace_all, round,
    simple_json_extract_float, simple_json_extract_int, simple_json_extract_string,
    simple_json_has, sip_hash64, stddev_pop, stddev_pop_stable, stddev_samp, stddev_samp_stable,
    substring, sum_if, sum_merge, sum_merge_state, sum_state, to_bool, to_date, to_date_time,
    to_date_time64, to_day_of_month, to_float32, to_float64, to_float64_or_null,
    to_float64_or_zero, to_hour, to_int8, to_int16, to_int32, to_int32_or_null, to_int64,
    to_int64_or_null, to_int64_or_zero, to_int128, to_int256, to_ipv4, to_ipv6, to_minute,
    to_month, to_start_of_day, to_start_of_hour, to_start_of_minute, to_start_of_month,
    to_start_of_year, to_string, to_uint8, to_uint16, to_uint32, to_uint32_or_null, to_uint64,
    to_uint64_or_null, to_uint64_or_zero, to_uint128, to_uint256, to_unix_timestamp, to_year,
    top_level_domain, try_base64_decode, unhex, uniq, uniq_exact, uniq_exact_if, uniq_exact_merge,
    uniq_exact_state, uniq_if, uniq_merge, uniq_state, upper, url_fragment, url_path,
    url_path_full, url_protocol, url_query_string, var_pop, var_pop_stable, var_samp,
    var_samp_stable, xx_hash64,
};
pub use grouping::{
    GroupByAll, GroupByModifier, GroupByModifierKind, Grouping, GroupingSets, cube, group_by_all,
    grouping, grouping_sets, rollup, with_totals,
};
pub use higher_order::{
    HigherOrderFunction, Lambda, array_all, array_count, array_exists, array_filter, array_map,
    array_map_as, lambda, lambda_params, lambda2, map_apply, map_filter,
};
pub use joins::{
    AliasedColumn, ClickHouseJoin, ClickHouseJoinBuilder, ClickHouseJoinDsl, JoinColumn, JoinKind,
    JoinModifier, JoinOn, JoinStrictness, JoinUsing, clickhouse_join, expr_as, join_column,
    source_column, source_column_as,
};
pub use json::{
    JsonPathFunction, JsonPathSegment, json_extract_bool_path, json_extract_float_path,
    json_extract_int_ci_path, json_extract_int_path, json_extract_raw_ci_path,
    json_extract_raw_path, json_extract_string_ci_path, json_extract_string_path,
    json_extract_uint_path,
};
pub use operators::{
    ClickHouseTextExpressionMethods, GlobalIn, GlobalInDsl, ILike, NotGlobalIn, NotGlobalInDsl,
    NotILike,
};
pub use ordering::{FillBound, NoFillBound, WithFill, with_fill};
pub use vectors::{
    VectorBytes, VectorBytesEncoding, VectorLiteral, vector_f32, vector_f32_binary, vector_f32_hex,
    vector_f32_le_bytes, vector_f32_le_hex, vector_f64, vector_f64_binary, vector_f64_hex,
    vector_f64_le_bytes, vector_f64_le_hex,
};
pub use window::{
    NoWindowBindings, NoWindowFrame, NoWindowOrder, NoWindowPartition, Over, OverDsl, OverWindow,
    QualifyQuery, RowsBetweenUnboundedPrecedingAndCurrentRow, WindowBinding, WindowFrame,
    WindowFrameBound, WindowFrameUnits, WindowOrder, WindowPartition, WindowQuery, WindowSpec,
    dense_rank, first_value, lag, lag_in_frame, last_value, lead, lead_in_frame, partition_by,
    qualify, rank, row_number, window, window_order_by,
};

/// ClickHouse SQL-type markers for use in `table!` declarations and explicit
/// `select` type annotations.
pub mod sql_types {
    pub use crate::types::{
        AggregateFunction, Array, BFloat16, DateTime64, Decimal32, Decimal64, Decimal128,
        Decimal256, Dynamic, Enum8, Enum16, IPv4, IPv6, Int8, Int128, Int256, Json, LowCardinality,
        Map, Nested, Nothing, Point, Ring, Tuple, UInt8, UInt16, UInt32, UInt64, UInt128, UInt256,
        Uuid, Variant,
    };
}
