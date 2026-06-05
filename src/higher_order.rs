//! ClickHouse higher-order array/map helpers with lambda fragments.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{
    AppearsOnTable, Expression, MixedAggregates, SelectableExpression, ValidGrouping,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{BigInt, Bool, SingleValue, SqlType};

use crate::backend::ClickHouse;
use crate::types::Array;

/// Build a single-argument ClickHouse lambda, e.g. `x -> x + 1`.
pub fn lambda(param: impl Into<String>, body: impl Into<String>) -> Lambda {
    lambda_params([param], body)
}

/// Build a two-argument ClickHouse lambda, e.g. `(k, v) -> v > 0`.
pub fn lambda2(
    first: impl Into<String>,
    second: impl Into<String>,
    body: impl Into<String>,
) -> Lambda {
    lambda_params([first.into(), second.into()], body)
}

/// Build a ClickHouse lambda from an arbitrary parameter list.
pub fn lambda_params<I, S>(params: I, body: impl Into<String>) -> Lambda
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Lambda {
        params: params.into_iter().map(Into::into).collect(),
        body: body.into(),
    }
}

/// Raw lambda expression used by higher-order ClickHouse functions.
#[derive(Debug, Clone)]
pub struct Lambda {
    params: Vec<String>,
    body: String,
}

/// A higher-order function call of the shape `function(lambda, expr)`.
#[derive(Debug, Clone)]
pub struct HigherOrderFunction<Expr, ST> {
    function: &'static str,
    lambda: Lambda,
    expr: Expr,
    _sql_type: PhantomData<ST>,
}

/// A higher-order function call of the shape `function(lambda, first, second)`.
#[derive(Debug, Clone)]
pub struct BinaryHigherOrderFunction<First, Second, ST> {
    function: &'static str,
    lambda: Lambda,
    first: First,
    second: Second,
    _sql_type: PhantomData<ST>,
}

/// Render `arrayMap(lambda, array)` returning the same array type as the input.
pub fn array_map<Expr>(lambda: Lambda, array: Expr) -> HigherOrderFunction<Expr, Expr::SqlType>
where
    Expr: Expression,
    Expr::SqlType: SqlType,
{
    HigherOrderFunction::new("arrayMap", lambda, array)
}

/// Render `arrayMap(lambda, array)` with an explicit result element type.
pub fn array_map_as<Out, Expr>(lambda: Lambda, array: Expr) -> HigherOrderFunction<Expr, Array<Out>>
where
    Out: SqlType + SingleValue,
    Expr: Expression,
{
    HigherOrderFunction::new("arrayMap", lambda, array)
}

/// Render `arrayFilter(lambda, array)`.
pub fn array_filter<Expr>(lambda: Lambda, array: Expr) -> HigherOrderFunction<Expr, Expr::SqlType>
where
    Expr: Expression,
    Expr::SqlType: SqlType,
{
    HigherOrderFunction::new("arrayFilter", lambda, array)
}

/// Render `arrayExists(lambda, array)`.
pub fn array_exists<Expr>(lambda: Lambda, array: Expr) -> HigherOrderFunction<Expr, Bool>
where
    Expr: Expression,
{
    HigherOrderFunction::new("arrayExists", lambda, array)
}

/// Render `arrayExists(lambda, first, second)` over two parallel arrays.
///
/// ClickHouse evaluates the two-parameter lambda once for each aligned pair of
/// elements and returns true if any pair matches. This keeps the array values as
/// ordinary Diesel expressions/binds while avoiding hand-written function-call
/// assembly around the lambda.
pub fn array_exists2<First, Second>(
    lambda: Lambda,
    first: First,
    second: Second,
) -> BinaryHigherOrderFunction<First, Second, Bool>
where
    First: Expression,
    Second: Expression,
{
    BinaryHigherOrderFunction::new("arrayExists", lambda, first, second)
}

/// Render `arrayAll(lambda, array)`.
pub fn array_all<Expr>(lambda: Lambda, array: Expr) -> HigherOrderFunction<Expr, Bool>
where
    Expr: Expression,
{
    HigherOrderFunction::new("arrayAll", lambda, array)
}

/// Render `arrayCount(lambda, array)`.
pub fn array_count<Expr>(lambda: Lambda, array: Expr) -> HigherOrderFunction<Expr, BigInt>
where
    Expr: Expression,
{
    HigherOrderFunction::new("arrayCount", lambda, array)
}

/// Render `mapApply(lambda, map)` returning the same map type as the input.
pub fn map_apply<Expr>(lambda: Lambda, map: Expr) -> HigherOrderFunction<Expr, Expr::SqlType>
where
    Expr: Expression,
    Expr::SqlType: SqlType,
{
    HigherOrderFunction::new("mapApply", lambda, map)
}

/// Render `mapFilter(lambda, map)`.
pub fn map_filter<Expr>(lambda: Lambda, map: Expr) -> HigherOrderFunction<Expr, Expr::SqlType>
where
    Expr: Expression,
    Expr::SqlType: SqlType,
{
    HigherOrderFunction::new("mapFilter", lambda, map)
}

impl<Expr, ST> HigherOrderFunction<Expr, ST> {
    fn new(function: &'static str, lambda: Lambda, expr: Expr) -> Self {
        Self {
            function,
            lambda,
            expr,
            _sql_type: PhantomData,
        }
    }
}

impl<First, Second, ST> BinaryHigherOrderFunction<First, Second, ST> {
    fn new(function: &'static str, lambda: Lambda, first: First, second: Second) -> Self {
        Self {
            function,
            lambda,
            first,
            second,
            _sql_type: PhantomData,
        }
    }
}

impl<Expr, ST> Expression for HigherOrderFunction<Expr, ST>
where
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<Expr, ST, GB> ValidGrouping<GB> for HigherOrderFunction<Expr, ST>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<First, Second, ST, GB> ValidGrouping<GB> for BinaryHigherOrderFunction<First, Second, ST>
where
    First: ValidGrouping<GB>,
    Second: ValidGrouping<GB>,
    First::IsAggregate: MixedAggregates<Second::IsAggregate>,
{
    type IsAggregate = <First::IsAggregate as MixedAggregates<Second::IsAggregate>>::Output;
}

impl<Expr, ST, QS> AppearsOnTable<QS> for HigherOrderFunction<Expr, ST>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Expr, ST, QS> SelectableExpression<QS> for HigherOrderFunction<Expr, ST> where
    Self: AppearsOnTable<QS>
{
}

impl<First, Second, ST> Expression for BinaryHigherOrderFunction<First, Second, ST>
where
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<First, Second, ST, QS> AppearsOnTable<QS> for BinaryHigherOrderFunction<First, Second, ST>
where
    First: AppearsOnTable<QS>,
    Second: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<First, Second, ST, QS> SelectableExpression<QS>
    for BinaryHigherOrderFunction<First, Second, ST>
where
    Self: AppearsOnTable<QS>,
{
}

impl<Expr, ST> QueryId for HigherOrderFunction<Expr, ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<First, Second, ST> QueryId for BinaryHigherOrderFunction<First, Second, ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl QueryFragment<ClickHouse> for Lambda {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.params.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse lambda requires at least one parameter".into(),
            ));
        }
        if self.body.trim().is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse lambda body must not be empty".into(),
            ));
        }

        if self.params.len() == 1 {
            validate_bare_identifier(&self.params[0], "lambda parameter")?;
            out.push_sql(&self.params[0]);
        } else {
            out.push_sql("(");
            for (idx, param) in self.params.iter().enumerate() {
                if idx > 0 {
                    out.push_sql(", ");
                }
                validate_bare_identifier(param, "lambda parameter")?;
                out.push_sql(param);
            }
            out.push_sql(")");
        }
        out.push_sql(" -> ");
        out.push_sql(&self.body);
        Ok(())
    }
}

impl<Expr, ST, DB> QueryFragment<DB> for HigherOrderFunction<Expr, ST>
where
    DB: Backend,
    Lambda: QueryFragment<DB>,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql(self.function);
        out.push_sql("(");
        self.lambda.walk_ast(out.reborrow())?;
        out.push_sql(", ");
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

impl<First, Second, ST, DB> QueryFragment<DB> for BinaryHigherOrderFunction<First, Second, ST>
where
    DB: Backend,
    Lambda: QueryFragment<DB>,
    First: QueryFragment<DB>,
    Second: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if self.lambda.params.len() != 2 {
            return Err(Error::QueryBuilderError(
                format!(
                    "ClickHouse {} over two arrays requires a two-parameter lambda",
                    self.function
                )
                .into(),
            ));
        }

        out.push_sql(self.function);
        out.push_sql("(");
        self.lambda.walk_ast(out.reborrow())?;
        out.push_sql(", ");
        self.first.walk_ast(out.reborrow())?;
        out.push_sql(", ");
        self.second.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

fn validate_bare_identifier(value: &str, kind: &str) -> QueryResult<()> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(Error::QueryBuilderError(
            format!("empty ClickHouse {kind}").into(),
        ));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse {kind}: {value:?}").into(),
        ));
    }
    if chars.any(|ch| !(ch == '_' || ch.is_ascii_alphanumeric())) {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse {kind}: {value:?}").into(),
        ));
    }
    Ok(())
}
