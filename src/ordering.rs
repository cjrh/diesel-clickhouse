//! ClickHouse-specific ordering expressions.

use diesel::backend::Backend;
use diesel::expression::{
    AppearsOnTable, AsExpression, Expression, SelectableExpression, ValidGrouping,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::QueryResult;
use diesel::sql_types::SqlType;

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
