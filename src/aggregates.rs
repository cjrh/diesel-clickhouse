//! ClickHouse aggregate expressions that cannot be represented as ordinary
//! function calls.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{
    AppearsOnTable, Expression, SelectableExpression, ValidGrouping, is_aggregate,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{Double, SingleValue, SqlType};

use crate::types::Array;

/// Generic renderer for ClickHouse's parametric aggregate syntax:
/// `function(param, ...)(expr)`.
#[derive(Debug, Clone)]
pub struct ParametricAggregate<Expr, ST> {
    function: &'static str,
    params: Vec<AggregateParam>,
    expr: Expr,
    _sql_type: PhantomData<ST>,
}

/// `quantile(level)(expr)` — ClickHouse's parametric approximate quantile.
pub type Quantile<Expr> = ParametricAggregate<Expr, Double>;

/// Build a `quantile(level)(expr)` aggregate expression.
pub fn quantile<Expr>(level: f64, expr: Expr) -> Quantile<Expr>
where
    Expr: Expression,
{
    ParametricAggregate::probability("quantile", [level], expr)
}

/// Build a `quantileExact(level)(expr)` aggregate expression.
pub fn quantile_exact<Expr>(level: f64, expr: Expr) -> ParametricAggregate<Expr, Double>
where
    Expr: Expression,
{
    ParametricAggregate::probability("quantileExact", [level], expr)
}

/// Build a `quantileTDigest(level)(expr)` aggregate expression.
pub fn quantile_tdigest<Expr>(level: f64, expr: Expr) -> ParametricAggregate<Expr, Double>
where
    Expr: Expression,
{
    ParametricAggregate::probability("quantileTDigest", [level], expr)
}

/// Build a `quantiles(level, ...)(expr)` aggregate expression.
pub fn quantiles<Expr, Levels>(
    levels: Levels,
    expr: Expr,
) -> ParametricAggregate<Expr, Array<Double>>
where
    Expr: Expression,
    Levels: IntoIterator<Item = f64>,
{
    ParametricAggregate::probability("quantiles", levels, expr)
}

/// Build a `topK(k)(expr)` aggregate expression.
pub fn top_k<Expr>(k: u64, expr: Expr) -> ParametricAggregate<Expr, Array<Expr::SqlType>>
where
    Expr: Expression,
{
    ParametricAggregate::positive_integer("topK", [k], expr)
}

#[derive(Debug, Clone, Copy)]
enum AggregateParam {
    Probability(f64),
    PositiveInteger(u64),
}

impl<Expr, ST> ParametricAggregate<Expr, ST>
where
    Expr: Expression,
{
    fn probability<Levels>(function: &'static str, levels: Levels, expr: Expr) -> Self
    where
        Levels: IntoIterator<Item = f64>,
    {
        Self {
            function,
            params: levels
                .into_iter()
                .map(AggregateParam::Probability)
                .collect(),
            expr,
            _sql_type: PhantomData,
        }
    }

    fn positive_integer<Values>(function: &'static str, values: Values, expr: Expr) -> Self
    where
        Values: IntoIterator<Item = u64>,
    {
        Self {
            function,
            params: values
                .into_iter()
                .map(AggregateParam::PositiveInteger)
                .collect(),
            expr,
            _sql_type: PhantomData,
        }
    }
}

impl<Expr, ST> Expression for ParametricAggregate<Expr, ST>
where
    Expr: Expression,
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<Expr, ST, GB> ValidGrouping<GB> for ParametricAggregate<Expr, ST> {
    type IsAggregate = is_aggregate::Yes;
}

impl<Expr, ST, QS> AppearsOnTable<QS> for ParametricAggregate<Expr, ST> where Self: Expression {}
impl<Expr, ST, QS> SelectableExpression<QS> for ParametricAggregate<Expr, ST> where
    Self: AppearsOnTable<QS>
{
}

impl<Expr, ST> QueryId for ParametricAggregate<Expr, ST>
where
    Expr: QueryId,
    ST: QueryId,
{
    type QueryId = ParametricAggregate<Expr::QueryId, ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr, ST, DB> QueryFragment<DB> for ParametricAggregate<Expr, ST>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if self.params.is_empty() {
            return Err(Error::QueryBuilderError(
                format!(
                    "ClickHouse {} requires at least one parameter",
                    self.function
                )
                .into(),
            ));
        }

        out.push_sql(self.function);
        out.push_sql("(");
        for (idx, param) in self.params.iter().enumerate() {
            if idx > 0 {
                out.push_sql(", ");
            }
            push_param(&mut out, *param)?;
        }
        out.push_sql(")(");
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

fn push_param<DB>(out: &mut AstPass<'_, '_, DB>, param: AggregateParam) -> QueryResult<()>
where
    DB: Backend,
{
    match param {
        AggregateParam::Probability(value) => {
            if !(0.0..=1.0).contains(&value) || !value.is_finite() {
                return Err(Error::QueryBuilderError(
                    format!(
                        "ClickHouse probability parameter must be finite and between 0 and 1, got {value}"
                    )
                    .into(),
                ));
            }
            out.push_sql(&value.to_string());
        }
        AggregateParam::PositiveInteger(value) => {
            if value == 0 {
                return Err(Error::QueryBuilderError(
                    "ClickHouse positive integer parameter must be greater than 0".into(),
                ));
            }
            out.push_sql(&value.to_string());
        }
    }
    Ok(())
}
