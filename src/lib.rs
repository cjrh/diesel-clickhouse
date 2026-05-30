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
mod functions;
mod grouping;
mod operators;
mod ordering;
mod serialize;
mod types;
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
pub use functions::{
    abs, any_last, any_value, arg_max, arg_min, array_concat, array_distinct, array_element,
    array_join, avg_if, ceil, concat, count_if, date_diff, date_trunc, empty, floor, greatest,
    group_array, group_array_if, has, has_all, has_any, if_, int_div, json_extract_bool,
    json_extract_float, json_extract_int, json_extract_raw, json_extract_string, least, length,
    lower, map_contains, map_keys, map_values, max_if, min_if, not_empty, position, regexp_match,
    replace_all, round, substring, sum_if, to_date, to_date_time, to_date_time64, to_day_of_month,
    to_float64, to_hour, to_int64, to_minute, to_month, to_start_of_day, to_start_of_hour,
    to_start_of_minute, to_start_of_month, to_start_of_year, to_string, to_uint64,
    to_unix_timestamp, to_year, uniq, uniq_exact, uniq_exact_if, uniq_if, upper,
};
pub use grouping::{
    GroupByAll, GroupByModifier, GroupByModifierKind, Grouping, GroupingSets, cube, group_by_all,
    grouping, grouping_sets, rollup, with_totals,
};
pub use operators::{GlobalIn, GlobalInDsl, NotGlobalIn, NotGlobalInDsl};
pub use ordering::{FillBound, NoFillBound, WithFill, with_fill};
pub use window::{
    NoWindowBindings, NoWindowFrame, NoWindowOrder, NoWindowPartition, Over, OverDsl, OverWindow,
    QualifyQuery, RowsBetweenUnboundedPrecedingAndCurrentRow, WindowBinding, WindowOrder,
    WindowPartition, WindowQuery, WindowSpec, dense_rank, first_value, lag, lag_in_frame,
    last_value, lead, lead_in_frame, partition_by, qualify, rank, row_number, window,
    window_order_by,
};

/// ClickHouse SQL-type markers for use in `table!` declarations and explicit
/// `select` type annotations.
pub mod sql_types {
    pub use crate::types::{
        Array, DateTime64, Int8, Json, LowCardinality, Map, Nothing, UInt8, UInt16, UInt32, UInt64,
        Uuid,
    };
}
