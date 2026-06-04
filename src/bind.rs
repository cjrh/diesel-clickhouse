//! Binding Rust values at a chosen ClickHouse SQL type.
//!
//! Diesel lets you write `column.eq(value)` only when `value` implements
//! `AsExpression<column::SqlType>`. Diesel ships those impls for its own
//! primitives against its own SQL types, but it cannot provide them for the
//! ClickHouse-only types in [`crate::sql_types`] (`UInt8`..`UInt128`, `Int8`,
//! `Int128`, ...), and neither can this crate nor a downstream app:
//!
//! Diesel's blanket `impl<T: Expression<SqlType = ST>, ST> AsExpression<ST> for T`
//! overlaps with any hand-written `impl AsExpression<UInt64> for u64`. Because
//! `u64` and `Expression` are both foreign to every crate except `diesel`, the
//! coherence checker must assume a future `impl Expression for u64` could exist,
//! so the hand-written impl is rejected (E0119) — even inside the crate that
//! owns `UInt64`. This is a fundamental limitation, not a missing impl.
//!
//! [`bind`] sidesteps it. Instead of teaching `u64` to be an expression, it wraps
//! the value in a node that *is already* an expression of the requested SQL type.
//! That node then satisfies `AsExpression` through the same blanket Diesel uses
//! for its own expressions, so it drops straight into any comparison, filter, or
//! select:
//!
//! ```ignore
//! use diesel_clickhouse::bind;
//! use diesel_clickhouse::sql_types::UInt64;
//!
//! // Before: untyped escape hatch, value supplied out of band.
//! events::id.gt(diesel::dsl::sql::<UInt64>("?"))
//!
//! // After: the value is bound and type-checked against the column.
//! events::id.gt(bind(after_id))
//! ```
//!
//! The target SQL type is normally inferred from the surrounding expression
//! (the column being compared), so a turbofish is rarely needed. When binding in
//! a position with no inferable type, name it explicitly: `bind::<UInt64, _>(x)`.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{
    AppearsOnTable, Expression, SelectableExpression, TypedExpressionType, ValidGrouping,
    is_aggregate,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::QueryResult;
use diesel::serialize::ToSql;
use diesel::sql_types::{HasSqlType, SqlType};

/// A Rust value rendered as a bound parameter typed as the ClickHouse SQL type
/// `ST`.
///
/// Construct one with [`bind`]; the fields are private because the value only
/// makes sense paired with its declared SQL type. It behaves as a single-value
/// expression of type `ST`, so it is usable anywhere Diesel accepts an
/// expression of that type (filters, comparisons, selects).
#[derive(Debug, Clone, Copy)]
pub struct BoundValue<ST, T> {
    value: T,
    _sql_type: PhantomData<ST>,
}

/// Bind a Rust value as a ClickHouse SQL parameter of type `ST`.
///
/// Use this to compare or filter against ClickHouse-only column types that
/// Diesel cannot bind directly, such as the unsigned integers:
///
/// ```ignore
/// silver_aspects::silver_aspect_id.gt(bind(after_id))
/// ```
///
/// `ST` is inferred from the surrounding expression whenever possible (here,
/// from the column's SQL type), so callers seldom write it. The value is sent
/// as a real bind parameter when executed through
/// [`AsyncClickHouseConnection`](crate::AsyncClickHouseConnection) and rendered
/// as `?` by [`to_sql`](crate::to_sql), exactly like any other Diesel bind.
pub fn bind<ST, T>(value: T) -> BoundValue<ST, T> {
    BoundValue {
        value,
        _sql_type: PhantomData,
    }
}

impl<ST, T> Expression for BoundValue<ST, T>
where
    ST: SqlType + TypedExpressionType,
{
    type SqlType = ST;
}

impl<ST, T, DB> QueryFragment<DB> for BoundValue<ST, T>
where
    DB: Backend + HasSqlType<ST>,
    T: ToSql<ST, DB>,
{
    fn walk_ast<'b>(&'b self, mut pass: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        pass.push_bind_param(&self.value)?;
        Ok(())
    }
}

impl<ST: QueryId, T> QueryId for BoundValue<ST, T> {
    type QueryId = BoundValue<ST::QueryId, ()>;
    const HAS_STATIC_QUERY_ID: bool = ST::HAS_STATIC_QUERY_ID;
}

// A bound value carries no column reference, so it is valid against any source
// table and groups as a non-aggregate everywhere — mirroring how Diesel treats
// its own bound parameters.
impl<ST, T, QS> AppearsOnTable<QS> for BoundValue<ST, T> where BoundValue<ST, T>: Expression {}

impl<ST, T, QS> SelectableExpression<QS> for BoundValue<ST, T> where
    BoundValue<ST, T>: AppearsOnTable<QS>
{
}

impl<ST, T, GB> ValidGrouping<GB> for BoundValue<ST, T> {
    type IsAggregate = is_aggregate::Never;
}
