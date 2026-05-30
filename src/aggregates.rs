//! ClickHouse aggregate expressions that cannot be represented as ordinary
//! function calls.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{
    AppearsOnTable, Expression, SelectableExpression, ValidGrouping, is_aggregate,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{Double, Nullable, SingleValue, SqlType};

use crate::types::{AggregateFunction, Array, Tuple, UInt64};

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

/// Builder for ClickHouse aggregate function names plus combinator suffixes.
#[derive(Debug, Clone)]
pub struct AggregateBuilder<ST> {
    function: String,
    _sql_type: PhantomData<ST>,
}

/// A ClickHouse aggregate call such as `sumIf(x, cond)` or `avgOrNullIf(x, cond)`.
#[derive(Debug, Clone)]
pub struct AggregateCall<Args, ST> {
    function: String,
    suffixes: Vec<String>,
    args: Args,
    _sql_type: PhantomData<ST>,
}

/// No arguments, useful for `count()` and `countIf(cond)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoAggregateArgs;

/// One aggregate argument.
#[derive(Debug, Clone, Copy)]
pub struct OneAggregateArg<Expr>(Expr);

/// Two aggregate arguments, e.g. `argMax(arg, value)`.
#[derive(Debug, Clone, Copy)]
pub struct TwoAggregateArgs<Left, Right>(Left, Right);

/// Existing aggregate arguments plus a trailing `If` condition.
#[derive(Debug, Clone, Copy)]
pub struct AggregateIfArgs<Args, Cond> {
    args: Args,
    cond: Cond,
}

/// `quantile(level)(expr)` — ClickHouse's parametric approximate quantile.
pub type Quantile<Expr> = ParametricAggregate<Expr, Double>;

/// SQL type of each tuple returned by ClickHouse `histogram`.
pub type HistogramBucket = Tuple<(Double, Double, UInt64)>;

/// `histogram(bins)(expr)` adaptive histogram aggregate.
pub type Histogram<Expr> = ParametricAggregate<Expr, Array<HistogramBucket>>;

/// Result type returned by `approx_top_sum` for one value/weight pair.
pub type ApproxTopSumItem<Value, Weight> = Tuple<(Value, Weight)>;

/// `approx_top_sum` result array element type.
pub type ApproxTopSumResult<ValueSql, WeightSql> = Array<ApproxTopSumItem<ValueSql, WeightSql>>;

/// Start a generic ClickHouse aggregate/combinator call.
///
/// The type parameter is the aggregate result type before result-shaping
/// combinators such as `OrNull` or `State` are applied.
pub fn aggregate<ST>(function: impl Into<String>) -> AggregateBuilder<ST>
where
    ST: SqlType + SingleValue,
{
    AggregateBuilder {
        function: function.into(),
        _sql_type: PhantomData,
    }
}

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

/// Build `approx_top_sum(n)(value, weight)`.
pub fn approx_top_sum<Value, Weight>(
    n: u64,
    value: Value,
    weight: Weight,
) -> BinaryParametricAggregate<Value, Weight, ApproxTopSumResult<Value::SqlType, Weight::SqlType>>
where
    Value: Expression,
    Weight: Expression,
{
    BinaryParametricAggregate::positive_integer("approx_top_sum", [n], value, weight)
}

/// Build `approx_top_sum(n, reserved)(value, weight)`.
pub fn approx_top_sum_with_reserved<Value, Weight>(
    n: u64,
    reserved: u64,
    value: Value,
    weight: Weight,
) -> BinaryParametricAggregate<Value, Weight, ApproxTopSumResult<Value::SqlType, Weight::SqlType>>
where
    Value: Expression,
    Weight: Expression,
{
    BinaryParametricAggregate::positive_integer("approx_top_sum", [n, reserved], value, weight)
}

impl<ST> AggregateBuilder<ST>
where
    ST: SqlType + SingleValue,
{
    /// Build `function()` with no arguments.
    pub fn no_args(self) -> AggregateCall<NoAggregateArgs, ST> {
        AggregateCall::new(self.function, NoAggregateArgs)
    }

    /// Build `function(expr)`.
    pub fn arg<Expr>(self, expr: Expr) -> AggregateCall<OneAggregateArg<Expr>, ST>
    where
        Expr: Expression,
    {
        AggregateCall::new(self.function, OneAggregateArg(expr))
    }

    /// Build `function(left, right)`.
    pub fn args<Left, Right>(
        self,
        left: Left,
        right: Right,
    ) -> AggregateCall<TwoAggregateArgs<Left, Right>, ST>
    where
        Left: Expression,
        Right: Expression,
    {
        AggregateCall::new(self.function, TwoAggregateArgs(left, right))
    }
}

impl<Args, ST> AggregateCall<Args, ST>
where
    ST: SqlType + SingleValue,
{
    fn new(function: String, args: Args) -> Self {
        Self {
            function,
            suffixes: Vec::new(),
            args,
            _sql_type: PhantomData,
        }
    }

    /// Append a custom aggregate combinator suffix such as `Distinct` or `ForEach`.
    pub fn combinator(mut self, suffix: impl Into<String>) -> Self {
        self.suffixes.push(suffix.into());
        self
    }

    /// Append `Distinct`.
    pub fn distinct(self) -> Self {
        self.combinator("Distinct")
    }

    /// Append `OrDefault`.
    pub fn or_default(self) -> Self {
        self.combinator("OrDefault")
    }

    /// Append `Merge`.
    pub fn merge(self) -> Self {
        self.combinator("Merge")
    }

    /// Append `OrNull` and change the Diesel SQL type to `Nullable<ST>`.
    pub fn or_null(mut self) -> AggregateCall<Args, Nullable<ST>> {
        self.suffixes.push("OrNull".to_string());
        AggregateCall {
            function: self.function,
            suffixes: self.suffixes,
            args: self.args,
            _sql_type: PhantomData,
        }
    }

    /// Append `State` and change the Diesel SQL type to `AggregateFunction<ST>`.
    pub fn state(mut self) -> AggregateCall<Args, AggregateFunction<ST>> {
        self.suffixes.push("State".to_string());
        AggregateCall {
            function: self.function,
            suffixes: self.suffixes,
            args: self.args,
            _sql_type: PhantomData,
        }
    }

    /// Append `MergeState` and change the Diesel SQL type to `AggregateFunction<ST>`.
    pub fn merge_state(mut self) -> AggregateCall<Args, AggregateFunction<ST>> {
        self.suffixes.push("MergeState".to_string());
        AggregateCall {
            function: self.function,
            suffixes: self.suffixes,
            args: self.args,
            _sql_type: PhantomData,
        }
    }
}

impl<Args, ST> AggregateCall<Args, ST> {
    /// Append `If` and add the condition as the last aggregate argument.
    pub fn if_<Cond>(mut self, cond: Cond) -> AggregateCall<AggregateIfArgs<Args, Cond>, ST>
    where
        Cond: Expression,
    {
        self.suffixes.push("If".to_string());
        AggregateCall {
            function: self.function,
            suffixes: self.suffixes,
            args: AggregateIfArgs {
                args: self.args,
                cond,
            },
            _sql_type: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AggregateParam {
    Probability(f64),
    PositiveInteger(u64),
}

trait AggregateArguments<DB>
where
    DB: Backend,
{
    fn is_empty(&self) -> bool;
    fn walk_args<'b>(&'b self, out: AstPass<'_, 'b, DB>) -> QueryResult<()>;
}

impl<DB> AggregateArguments<DB> for NoAggregateArgs
where
    DB: Backend,
{
    fn is_empty(&self) -> bool {
        true
    }

    fn walk_args<'b>(&'b self, _out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Expr, DB> AggregateArguments<DB> for OneAggregateArg<Expr>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_args<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        self.0.walk_ast(out.reborrow())
    }
}

impl<Left, Right, DB> AggregateArguments<DB> for TwoAggregateArgs<Left, Right>
where
    DB: Backend,
    Left: QueryFragment<DB>,
    Right: QueryFragment<DB>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_args<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        self.0.walk_ast(out.reborrow())?;
        out.push_sql(", ");
        self.1.walk_ast(out.reborrow())
    }
}

impl<Args, Cond, DB> AggregateArguments<DB> for AggregateIfArgs<Args, Cond>
where
    DB: Backend,
    Args: AggregateArguments<DB>,
    Cond: QueryFragment<DB>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_args<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if !self.args.is_empty() {
            self.args.walk_args(out.reborrow())?;
            out.push_sql(", ");
        }
        self.cond.walk_ast(out.reborrow())
    }
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

    fn positive_integer<Values>(
        function: &'static str,
        values: Values,
        left: Left,
        right: Right,
    ) -> Self
    where
        Values: IntoIterator<Item = u64>,
    {
        Self {
            function,
            params: values
                .into_iter()
                .map(AggregateParam::PositiveInteger)
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

impl<Args, ST> Expression for AggregateCall<Args, ST>
where
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

impl<Args, ST, GB> ValidGrouping<GB> for AggregateCall<Args, ST> {
    type IsAggregate = is_aggregate::Yes;
}

impl<Expr, ST, QS> AppearsOnTable<QS> for ParametricAggregate<Expr, ST> where Self: Expression {}
impl<Left, Right, ST, QS> AppearsOnTable<QS> for BinaryParametricAggregate<Left, Right, ST> where
    Self: Expression
{
}
impl<Args, ST, QS> AppearsOnTable<QS> for AggregateCall<Args, ST> where Self: Expression {}
impl<Expr, ST, QS> SelectableExpression<QS> for ParametricAggregate<Expr, ST> where
    Self: AppearsOnTable<QS>
{
}
impl<Left, Right, ST, QS> SelectableExpression<QS> for BinaryParametricAggregate<Left, Right, ST> where
    Self: AppearsOnTable<QS>
{
}
impl<Args, ST, QS> SelectableExpression<QS> for AggregateCall<Args, ST> where
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

impl<Args, ST> QueryId for AggregateCall<Args, ST> {
    type QueryId = ();
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

impl<Args, ST, DB> QueryFragment<DB> for AggregateCall<Args, ST>
where
    DB: Backend,
    Args: AggregateArguments<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        validate_aggregate_name(&self.function, "aggregate function")?;
        out.push_sql(&self.function);
        for suffix in &self.suffixes {
            validate_aggregate_name(suffix, "aggregate combinator")?;
            out.push_sql(suffix);
        }
        out.push_sql("(");
        self.args.walk_args(out.reborrow())?;
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

fn validate_aggregate_name(value: &str, label: &'static str) -> QueryResult<()> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse {label} must not be empty").into(),
        ));
    };
    if !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse {label}: {value:?}").into(),
        ));
    }
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
