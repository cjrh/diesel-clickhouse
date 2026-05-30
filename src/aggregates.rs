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

use crate::types::{Array, Tuple, UInt64};

/// Generic renderer for ClickHouse's parametric aggregate syntax:
/// `function(param, ...)(expr)`.
#[derive(Debug, Clone)]
pub struct ParametricAggregate<Expr, ST> {
    function: &'static str,
    params: Vec<AggregateParam>,
    expr: Expr,
    _sql_type: PhantomData<ST>,
}

/// Renderer for ClickHouse parametric aggregates with two expression arguments:
/// `function(param, ...)(left, right)`.
#[derive(Debug, Clone)]
pub struct BinaryParametricAggregate<Left, Right, ST> {
    function: &'static str,
    params: Vec<AggregateParam>,
    left: Left,
    right: Right,
    _sql_type: PhantomData<ST>,
}

/// `quantile(level)(expr)` — ClickHouse's parametric approximate quantile.
pub type Quantile<Expr> = ParametricAggregate<Expr, Double>;

/// SQL type of each tuple returned by ClickHouse `histogram`.
pub type HistogramBucket = Tuple<(Double, Double, UInt64)>;

/// `histogram(bins)(expr)` adaptive histogram aggregate.
pub type Histogram<Expr> = ParametricAggregate<Expr, Array<HistogramBucket>>;

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

/// Build a `quantileTiming(level)(expr)` aggregate expression.
pub fn quantile_timing<Expr>(level: f64, expr: Expr) -> ParametricAggregate<Expr, Double>
where
    Expr: Expression,
{
    ParametricAggregate::probability("quantileTiming", [level], expr)
}

/// Build a `quantileDeterministic(level)(expr, determinator)` aggregate expression.
pub fn quantile_deterministic<Expr, Determinator>(
    level: f64,
    expr: Expr,
    determinator: Determinator,
) -> BinaryParametricAggregate<Expr, Determinator, Double>
where
    Expr: Expression,
    Determinator: Expression,
{
    BinaryParametricAggregate::probability("quantileDeterministic", [level], expr, determinator)
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

/// Build a `quantilesTiming(level, ...)(expr)` aggregate expression.
pub fn quantiles_timing<Expr, Levels>(
    levels: Levels,
    expr: Expr,
) -> ParametricAggregate<Expr, Array<Double>>
where
    Expr: Expression,
    Levels: IntoIterator<Item = f64>,
{
    ParametricAggregate::probability("quantilesTiming", levels, expr)
}

/// Build a `histogram(bins)(expr)` aggregate expression.
pub fn histogram<Expr>(bins: u64, expr: Expr) -> Histogram<Expr>
where
    Expr: Expression,
{
    ParametricAggregate::positive_integer("histogram", [bins], expr)
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

impl<Left, Right, ST> BinaryParametricAggregate<Left, Right, ST>
where
    Left: Expression,
    Right: Expression,
{
    fn probability<Levels>(function: &'static str, levels: Levels, left: Left, right: Right) -> Self
    where
        Levels: IntoIterator<Item = f64>,
    {
        Self {
            function,
            params: levels
                .into_iter()
                .map(AggregateParam::Probability)
                .collect(),
            left,
            right,
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

impl<Left, Right, ST> Expression for BinaryParametricAggregate<Left, Right, ST>
where
    Left: Expression,
    Right: Expression,
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<Expr, ST, GB> ValidGrouping<GB> for ParametricAggregate<Expr, ST> {
    type IsAggregate = is_aggregate::Yes;
}

impl<Left, Right, ST, GB> ValidGrouping<GB> for BinaryParametricAggregate<Left, Right, ST> {
    type IsAggregate = is_aggregate::Yes;
}

impl<Expr, ST, QS> AppearsOnTable<QS> for ParametricAggregate<Expr, ST> where Self: Expression {}
impl<Left, Right, ST, QS> AppearsOnTable<QS> for BinaryParametricAggregate<Left, Right, ST> where
    Self: Expression
{
}
impl<Expr, ST, QS> SelectableExpression<QS> for ParametricAggregate<Expr, ST> where
    Self: AppearsOnTable<QS>
{
}
impl<Left, Right, ST, QS> SelectableExpression<QS> for BinaryParametricAggregate<Left, Right, ST> where
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

impl<Left, Right, ST> QueryId for BinaryParametricAggregate<Left, Right, ST>
where
    Left: QueryId,
    Right: QueryId,
    ST: QueryId,
{
    type QueryId = BinaryParametricAggregate<Left::QueryId, Right::QueryId, ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr, ST, DB> QueryFragment<DB> for ParametricAggregate<Expr, ST>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        push_parametric_prefix(&mut out, self.function, &self.params)?;
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

impl<Left, Right, ST, DB> QueryFragment<DB> for BinaryParametricAggregate<Left, Right, ST>
where
    DB: Backend,
    Left: QueryFragment<DB>,
    Right: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        push_parametric_prefix(&mut out, self.function, &self.params)?;
        self.left.walk_ast(out.reborrow())?;
        out.push_sql(", ");
        self.right.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

fn push_parametric_prefix<DB>(
    out: &mut AstPass<'_, '_, DB>,
    function: &'static str,
    params: &[AggregateParam],
) -> QueryResult<()>
where
    DB: Backend,
{
    if params.is_empty() {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse {function} requires at least one parameter").into(),
        ));
    }

    out.push_sql(function);
    out.push_sql("(");
    for (idx, param) in params.iter().enumerate() {
        if idx > 0 {
            out.push_sql(", ");
        }
        push_param(out, *param)?;
    }
    out.push_sql(")(");
    Ok(())
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
