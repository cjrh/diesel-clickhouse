//! ClickHouse cast expressions whose return SQL type is chosen by the caller.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{AppearsOnTable, Expression, SelectableExpression, ValidGrouping};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{Nullable, SingleValue, SqlType};

/// Render `CAST(expr, 'Type')` and mark the expression as SQL type `ST`.
pub fn cast<ST, Expr>(expr: Expr, target: impl Into<String>) -> CastFunction<Expr, ST>
where
    Expr: Expression,
    ST: SqlType + SingleValue,
{
    CastFunction::new("CAST", expr, target)
}

/// Render `accurateCast(expr, 'Type')` and mark the expression as SQL type `ST`.
pub fn accurate_cast<ST, Expr>(expr: Expr, target: impl Into<String>) -> CastFunction<Expr, ST>
where
    Expr: Expression,
    ST: SqlType + SingleValue,
{
    CastFunction::new("accurateCast", expr, target)
}

/// Render `accurateCastOrNull(expr, 'Type')` as `Nullable<ST>`.
pub fn accurate_cast_or_null<ST, Expr>(
    expr: Expr,
    target: impl Into<String>,
) -> CastFunction<Expr, Nullable<ST>>
where
    Expr: Expression,
    ST: SqlType + SingleValue,
{
    CastFunction::new("accurateCastOrNull", expr, target)
}

/// Render `accurateCastOrDefault(expr, 'Type')` and mark the expression as `ST`.
pub fn accurate_cast_or_default<ST, Expr>(
    expr: Expr,
    target: impl Into<String>,
) -> CastFunction<Expr, ST>
where
    Expr: Expression,
    ST: SqlType + SingleValue,
{
    CastFunction::new("accurateCastOrDefault", expr, target)
}

/// A ClickHouse cast-like function call with caller-provided result SQL type.
#[derive(Debug, Clone)]
pub struct CastFunction<Expr, ST> {
    function: &'static str,
    expr: Expr,
    target: String,
    _sql_type: PhantomData<ST>,
}

impl<Expr, ST> CastFunction<Expr, ST> {
    fn new(function: &'static str, expr: Expr, target: impl Into<String>) -> Self {
        Self {
            function,
            expr,
            target: target.into(),
            _sql_type: PhantomData,
        }
    }
}

impl<Expr, ST> Expression for CastFunction<Expr, ST>
where
    Expr: Expression,
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<Expr, ST, GB> ValidGrouping<GB> for CastFunction<Expr, ST>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<Expr, ST, QS> AppearsOnTable<QS> for CastFunction<Expr, ST>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Expr, ST, QS> SelectableExpression<QS> for CastFunction<Expr, ST> where Self: AppearsOnTable<QS>
{}

impl<Expr, ST> QueryId for CastFunction<Expr, ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr, ST, DB> QueryFragment<DB> for CastFunction<Expr, ST>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if self.target.trim().is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse cast target type must not be empty".into(),
            ));
        }

        out.push_sql(self.function);
        out.push_sql("(");
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(", ");
        push_string_literal(&mut out, &self.target);
        out.push_sql(")");
        Ok(())
    }
}

fn push_string_literal<DB>(out: &mut AstPass<'_, '_, DB>, value: &str)
where
    DB: Backend,
{
    out.push_sql("'");
    let mut remaining = value;
    while let Some(idx) = remaining.find('\'') {
        out.push_sql(&remaining[..idx]);
        out.push_sql("''");
        remaining = &remaining[idx + 1..];
    }
    out.push_sql(remaining);
    out.push_sql("'");
}
