//! Applying a filter only when a runtime condition holds.
//!
//! On most Diesel backends an optional filter is written by boxing the query and
//! adding `.filter(...)` conditionally. The lightweight ClickHouse backend does
//! not support boxed queries (`.into_boxed()`), so that pattern is unavailable,
//! and callers fall back to a SQL sentinel such as `(? = '' OR col = ?)` — the
//! empty value disables the filter — which references the value twice and leaks
//! an untyped `sql::<Text>("?")` placeholder.
//!
//! [`when`] expresses the same intent directly. It wraps a boolean predicate and
//! a runtime flag: when the flag is set the predicate renders normally; when it
//! is clear the node renders the constant `1` (always true), so the surrounding
//! `WHERE` matches every row. The predicate's bind values are emitted only when
//! it is active, so the value is referenced once and stays fully typed:
//!
//! ```ignore
//! use diesel_clickhouse::when;
//!
//! // Filter by `type` only when the caller supplied a non-empty value.
//! let query = final_table(silver_aspects::table)
//!     .select(source_column(silver_aspects::silver_aspect_id))
//!     .filter(silver_aspects::tenant_id.eq(tenant_id))
//!     .filter(when(!aspect_type.is_empty(), silver_aspects::type_.eq(aspect_type)));
//! ```
//!
//! Because the rendered SQL depends on the runtime flag, a `when(..)` node has no
//! static query id — the same trade-off Diesel's own boxed queries make.

use diesel::backend::Backend;
use diesel::expression::{AppearsOnTable, Expression, SelectableExpression, ValidGrouping};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::QueryResult;
use diesel::sql_types::Bool;

/// Apply `predicate` only when `enabled` is true; otherwise match every row.
///
/// The result is a boolean expression usable anywhere Diesel accepts one — most
/// often inside `.filter(...)` or `.on(...)`. When `enabled` is false the node
/// renders as the always-true constant `1` and binds nothing, so an absent
/// optional filter adds no constraint and no bind values.
pub fn when<P>(enabled: bool, predicate: P) -> When<P> {
    When { enabled, predicate }
}

/// A predicate that is applied conditionally; see [`when`].
#[derive(Debug, Clone, Copy)]
pub struct When<P> {
    enabled: bool,
    predicate: P,
}

impl<P> Expression for When<P>
where
    P: Expression<SqlType = Bool>,
{
    type SqlType = Bool;
}

impl<P, DB> QueryFragment<DB> for When<P>
where
    DB: Backend,
    P: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if self.enabled {
            self.predicate.walk_ast(out.reborrow())?;
        } else {
            // `1` is ClickHouse's canonical truthy literal, so a disabled filter
            // contributes no constraint to the surrounding boolean context.
            out.push_sql("1");
        }
        Ok(())
    }
}

// The rendered SQL varies with `enabled`, so the node cannot claim a static
// query id — matching how Diesel treats boxed queries and raw SQL literals.
impl<P> QueryId for When<P> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<P, QS> AppearsOnTable<QS> for When<P>
where
    When<P>: Expression,
    P: AppearsOnTable<QS>,
{
}

impl<P, QS> SelectableExpression<QS> for When<P>
where
    When<P>: AppearsOnTable<QS>,
    P: SelectableExpression<QS>,
{
}

impl<P, GB> ValidGrouping<GB> for When<P>
where
    P: ValidGrouping<GB>,
{
    type IsAggregate = P::IsAggregate;
}
