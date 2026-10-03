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
//! ## `IN` lists: `in_list`
//!
//! `column.eq_any(vec)` is Diesel's `IN (?, ?, ...)`. It needs
//! `T: AsExpression<column::SqlType>` for the element type `T`, so it works
//! for the types Diesel itself covers (`i16`, `i32`, `i64`, `f32`, `f64`, `bool`,
//! `String`, `&str`) and **does not compile** for the ClickHouse-only types
//! (`Vec<u64>` against a `UInt64` column), for `Uuid`, or for [`bind`] elements
//! (`Many<ST, I>` needs `I: ToSql<ST>`, which a `BoundValue` is not). The error
//! reads `u64: AsExpression<UInt64>` is not satisfied.
//!
//! Use [`in_list`] instead. It sends the whole list as one `Array` bind and
//! renders `(column IN ?)`, so it works for every element type that has an
//! array `ToSql`, and a huge list is one parameter, not thousands:
//!
//! ```ignore
//! use diesel_clickhouse::in_list;
//!
//! // IN (...) over a UInt64 column; `eq_any(ids)` would not compile.
//! events::table.filter(in_list(events::id, ids))
//! ```
//!
//! Past the connection's URI budget (see
//! [`AsyncClickHouseConnection::with_max_param_uri_bytes`](crate::AsyncClickHouseConnection::with_max_param_uri_bytes))
//! the array moves to the request body, so list size no longer hits "uri too
//! long". The remaining limit is ClickHouse's own `max_query_size`.
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
use diesel::sql_types::is_nullable;
use diesel::sql_types::{Bool, HasSqlType, SqlType};

use crate::backend::ClickHouse;
use crate::types::Array;

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
/// For `IN` lists use [`in_list`]: `eq_any` does not accept these types.
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

/// `column IN (values...)` as a single array bind: renders `(column IN ?)`.
///
/// Construct one with [`in_list`].
#[derive(Debug, Clone)]
pub struct InList<Col, T> {
    column: Col,
    values: Vec<T>,
}

/// Filter `column` to any of `values`, sent as one `Array` bind.
///
/// `column IN (values)`, rendered `(column IN ?)`. An empty list matches
/// nothing.
///
/// It renders `IN`, not `has(?, column)`. Both prune granules the same way, but
/// before ClickHouse 26.6 (which adds `optimize_rewrite_has_to_in`) `has` with a
/// constant array compares each row against the whole array, O(rows × ids).
/// `IN` builds a hash set once. With 3,000 UUIDs over 1.5M rows on 26.3 that is
/// 1.5 s against 0.09 s.
///
/// ```ignore
/// use diesel_clickhouse::in_list;
///
/// events::table.filter(in_list(events::id, vec![1_u64, 2, 3]))
/// ```
///
/// # Why not `eq_any`?
///
/// `column.eq_any(vec)` needs `T: AsExpression<column::SqlType>` for the element
/// type `T`. That
/// holds for the types Diesel itself covers (`i16`, `i32`, `i64`, `f32`,
/// `f64`, `bool`, `String`, `&str`) and **does not compile** for
/// ClickHouse-only types (`Vec<u64>` against a `UInt64` column, `UInt128`,
/// `Uuid`, ...). The error reads ``u64: AsExpression<UInt64>` is not
/// satisfied``. Diesel's coherence rules make that impl impossible to write
/// (see [`bind`]). Wrapping each element in [`bind`] does not help either:
/// `eq_any` needs `ToSql` elements, which a [`BoundValue`] is not.
///
/// `in_list` has no such limit: the element type comes from the column, and
/// `Vec<T>` only has to serialize as `Array<column::SqlType>`. `Uuid` columns
/// take `Vec<String>` / `Vec<&str>` (canonical UUID text).
///
/// # Nullable columns
///
/// Not supported: a `Nullable<_>` column is a compile error. Array binds do not
/// carry element nullability, so `None` would be read as the default value
/// (`0`, `''`) and match rows it should not. Pass a non-null column or
/// expression, e.g. `assumeNotNull(column)` after an `IS NOT NULL` filter.
///
/// # Large lists
///
/// The list is one parameter, not thousands. Past the connection's URI budget
/// (see
/// [`AsyncClickHouseConnection::with_max_param_uri_bytes`](crate::AsyncClickHouseConnection::with_max_param_uri_bytes))
/// it moves to the request body, so size does not hit `uri too long`. The
/// remaining limit is ClickHouse's own `max_query_size`.
pub fn in_list<Col, T>(column: Col, values: Vec<T>) -> InList<Col, T>
where
    Col: Expression,
    Col::SqlType: SqlType<IsNull = is_nullable::NotNull>,
{
    InList { column, values }
}

impl<Col, T> Expression for InList<Col, T>
where
    Col: Expression,
{
    type SqlType = Bool;
}

impl<Col, T> QueryFragment<ClickHouse> for InList<Col, T>
where
    Col: Expression + QueryFragment<ClickHouse>,
    Col::SqlType: SqlType,
    ClickHouse: HasSqlType<Array<Col::SqlType>>,
    Vec<T>: ToSql<Array<Col::SqlType>, ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut pass: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        // Parenthesized so it stays one operand inside a larger expression,
        // as the `has(…)` call it replaced was.
        pass.push_sql("(");
        self.column.walk_ast(pass.reborrow())?;
        pass.push_sql(" IN ");
        pass.push_bind_param::<Array<Col::SqlType>, _>(&self.values)?;
        pass.push_sql(")");
        Ok(())
    }
}

impl<Col: QueryId, T> QueryId for InList<Col, T> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Col, T, QS> AppearsOnTable<QS> for InList<Col, T>
where
    Col: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Col, T, QS> SelectableExpression<QS> for InList<Col, T> where Self: AppearsOnTable<QS> {}

impl<Col, T, GB> ValidGrouping<GB> for InList<Col, T>
where
    Col: ValidGrouping<GB>,
{
    type IsAggregate = Col::IsAggregate;
}
