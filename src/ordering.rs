//! ClickHouse-specific ordering expressions.

use diesel::backend::Backend;
use std::marker::PhantomData;

use diesel::expression::{
    AppearsOnTable, AsExpression, Expression, SelectableExpression, TypedExpressionType,
    ValidGrouping, is_aggregate,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::SqlType;

use crate::backend::ClickHouse;

/// Wrap an `ORDER BY` expression with ClickHouse's `WITH FILL` modifier.
pub fn with_fill<Expr>(expr: Expr) -> WithFill<Expr>
where
    Expr: Expression,
{
    WithFill {
        expr,
        from: NoFillBound,
        to: NoFillBound,
        step: NoFillBound,
    }
}

/// Reference a ClickHouse SELECT alias as a typed expression.
///
/// ClickHouse lets `ORDER BY` and `GROUP BY` refer to aliases introduced in the
/// same select list. `alias_ref::<ST>("score")` renders the alias as a quoted
/// identifier after validating it is a simple ClickHouse identifier, avoiding
/// unchecked `sql::<ST>("score")` strings for this common pattern.
pub fn alias_ref<ST>(alias: impl Into<String>) -> AliasRef<ST> {
    AliasRef {
        alias: alias.into(),
        _sql_type: PhantomData,
    }
}

/// A typed reference to a SELECT-list alias.
#[derive(Debug, Clone)]
pub struct AliasRef<ST> {
    alias: String,
    _sql_type: PhantomData<ST>,
}

/// `expr WITH FILL [FROM x] [TO y] [STEP z]`.
#[derive(Debug, Clone, Copy)]
pub struct WithFill<Expr, From = NoFillBound, To = NoFillBound, Step = NoFillBound> {
    expr: Expr,
    from: From,
    to: To,
    step: Step,
}

/// Missing `WITH FILL` bound.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFillBound;

/// One optional `WITH FILL` bound.
#[derive(Debug, Clone, Copy)]
pub struct FillBound<Expr> {
    keyword: &'static str,
    expr: Expr,
}

impl<ST> Expression for AliasRef<ST>
where
    ST: SqlType + TypedExpressionType,
{
    type SqlType = ST;
}

impl<ST, QS> AppearsOnTable<QS> for AliasRef<ST> where AliasRef<ST>: Expression {}

impl<ST, QS> SelectableExpression<QS> for AliasRef<ST> where AliasRef<ST>: AppearsOnTable<QS> {}

impl<ST, GB> ValidGrouping<GB> for AliasRef<ST> {
    type IsAggregate = is_aggregate::Never;
}

impl<ST> QueryId for AliasRef<ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<ST> QueryFragment<ClickHouse> for AliasRef<ST> {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        validate_bare_identifier(&self.alias, "alias reference")?;
        out.push_identifier(&self.alias)?;
        Ok(())
    }
}

impl<Expr, From, To, Step> WithFill<Expr, From, To, Step>
where
    Expr: Expression,
    Expr::SqlType: SqlType,
{
    /// Add `FROM expr`.
    pub fn from<Value>(self, value: Value) -> WithFill<Expr, FillBound<Value::Expression>, To, Step>
    where
        Value: AsExpression<Expr::SqlType>,
    {
        WithFill {
            expr: self.expr,
            from: FillBound {
                keyword: " FROM ",
                expr: value.as_expression(),
            },
            to: self.to,
            step: self.step,
        }
    }

    /// Add `TO expr`.
    pub fn to<Value>(self, value: Value) -> WithFill<Expr, From, FillBound<Value::Expression>, Step>
    where
        Value: AsExpression<Expr::SqlType>,
    {
        WithFill {
            expr: self.expr,
            from: self.from,
            to: FillBound {
                keyword: " TO ",
                expr: value.as_expression(),
            },
            step: self.step,
        }
    }

    /// Add `STEP expr`.
    pub fn step<Value>(self, value: Value) -> WithFill<Expr, From, To, FillBound<Value::Expression>>
    where
        Value: AsExpression<Expr::SqlType>,
    {
        WithFill {
            expr: self.expr,
            from: self.from,
            to: self.to,
            step: FillBound {
                keyword: " STEP ",
                expr: value.as_expression(),
            },
        }
    }
}

impl<Expr, From, To, Step> Expression for WithFill<Expr, From, To, Step>
where
    Expr: Expression,
{
    type SqlType = Expr::SqlType;
}

impl<Expr, From, To, Step, GB> ValidGrouping<GB> for WithFill<Expr, From, To, Step>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<Expr, From, To, Step, QS> AppearsOnTable<QS> for WithFill<Expr, From, To, Step>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Expr, From, To, Step, QS> SelectableExpression<QS> for WithFill<Expr, From, To, Step> where
    Self: AppearsOnTable<QS>
{
}

impl<Expr, From, To, Step> QueryId for WithFill<Expr, From, To, Step>
where
    Expr: QueryId,
{
    type QueryId = WithFill<Expr::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<DB> QueryFragment<DB> for NoFillBound
where
    DB: Backend,
{
    fn walk_ast<'b>(&'b self, _out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Expr, DB> QueryFragment<DB> for FillBound<Expr>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql(self.keyword);
        self.expr.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl<Expr, From, To, Step, DB> QueryFragment<DB> for WithFill<Expr, From, To, Step>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
    From: QueryFragment<DB>,
    To: QueryFragment<DB>,
    Step: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(" WITH FILL");
        self.from.walk_ast(out.reborrow())?;
        self.to.walk_ast(out.reborrow())?;
        self.step.walk_ast(out.reborrow())?;
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
