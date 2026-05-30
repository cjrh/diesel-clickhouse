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
//! This crate currently focuses on **building ClickHouse SQL from Diesel ASTs**.
//! It does not yet provide a Diesel [`Connection`](diesel::Connection)
//! implementation.  Execute the rendered SQL through your ClickHouse client of
//! choice, or use the expression/fragment types in a future connection adapter.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/diesel-clickhouse/0.1.0")]

mod aggregates;
mod backend;
mod clauses;
mod ddl;
mod functions;
mod grouping;
mod higher_order;
mod joins;
mod operators;
mod ordering;
mod serialize;
mod types;
mod vectors;
mod window;

pub use aggregates::{
    ParametricAggregate, Quantile, quantile, quantile_exact, quantile_tdigest, quantiles, top_k,
};
pub use backend::{ClickHouse, ClickHouseQueryBuilder, ClickHouseTypeMetadata, to_sql};
pub use clauses::{
    ArrayJoin, ArrayJoinKind, ClickHouseQueryDsl, Final, Format, FormattedQuery, LimitBy,
    LimitWithTies, NoSampleOffset, NoWithBindings, Prewhere, Sample, SampleOffset, Setting,
    SettingValue, SettingsQuery, WithBinding, WithCteBinding, WithQuery, array_join_clause,
    array_join_clause_as, final_table, format, left_array_join_clause, left_array_join_clause_as,
    limit_by_col, prewhere, sample, sample_offset, settings, with_alias, with_cte,
    with_materialized_cte, with_ties,
};
pub use ddl::{
    AlterTable, BufferEngine, Column, CreateMaterializedView, CreateMaterializedViewBuilder,
    CreateTable, DataType, DistributedEngine, EngineSetting, EngineSettingValue, IndexType,
    MergeTree, NestedField, TableEngine, TableIndex, TableProjection, VectorDistanceFunction,
    VectorIndexAlgorithm, VectorQuantization, VectorSimilarityIndex, aggregating_merge_tree,
    alter_table, buffer, collapsing_merge_tree, create_materialized_view, create_table,
    distributed, merge_tree, projection, replacing_merge_tree, replacing_merge_tree_with,
    summing_merge_tree, summing_merge_tree_with, vector_similarity_index,
    versioned_collapsing_merge_tree,
};
pub use functions::{
    abs, any_last, any_value, arg_max, arg_min, array_concat, array_distinct, array_element,
    array_join, avg_if, avg_merge, avg_merge_state, avg_state, base64_decode, base64_encode, ceil,
    city_hash64, concat, cosine_distance, count_if, count_merge, count_merge_state, count_state,
    cut_query_string, date_diff, date_trunc, domain, domain_without_www, empty, farm_fingerprint64,
    finalize_aggregation, first_significant_subdomain, floor, greatest, group_array,
    group_array_if, group_array_merge, group_array_state, has, has_all, has_any, hex, if_, int_div,
    ipv4_num_to_string, ipv4_string_to_num, ipv6_num_to_string, is_ipv4_string, is_ipv6_string,
    json_extract_bool, json_extract_float, json_extract_int, json_extract_raw, json_extract_string,
    l1_distance, l1_norm, l2_distance, l2_norm, least, length, linf_distance, linf_norm, lower,
    map_contains, map_from_arrays, map_keys, map_values, max_if, max_merge, max_state, min_if,
    min_merge, min_state, not_empty, position, regexp_match, replace_all, round, sip_hash64,
    substring, sum_if, sum_merge, sum_merge_state, sum_state, to_date, to_date_time,
    to_date_time64, to_day_of_month, to_float64, to_hour, to_int64, to_ipv4, to_ipv6, to_minute,
    to_month, to_start_of_day, to_start_of_hour, to_start_of_minute, to_start_of_month,
    to_start_of_year, to_string, to_uint64, to_unix_timestamp, to_year, top_level_domain,
    try_base64_decode, unhex, uniq, uniq_exact, uniq_exact_if, uniq_exact_merge, uniq_exact_state,
    uniq_if, uniq_merge, uniq_state, upper, url_fragment, url_path, url_path_full, url_protocol,
    url_query_string, xx_hash64,
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
    ClickHouseJoin, ClickHouseJoinBuilder, ClickHouseJoinDsl, JoinKind, JoinModifier, JoinOn,
    JoinStrictness, JoinUsing, clickhouse_join,
};
pub use operators::{GlobalIn, GlobalInDsl, NotGlobalIn, NotGlobalInDsl};
pub use ordering::{FillBound, NoFillBound, WithFill, with_fill};
pub use vectors::{VectorLiteral, vector_f32, vector_f64};
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
        Decimal256, Enum8, Enum16, IPv4, IPv6, Int8, Int128, Int256, Json, LowCardinality, Map,
        Nested, Nothing, Tuple, UInt8, UInt16, UInt32, UInt64, UInt128, UInt256, Uuid,
    };
}
