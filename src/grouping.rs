//! ClickHouse `GROUP BY` modifiers expressed as Diesel grouping expressions.

use diesel::backend::Backend;
use diesel::expression::{
    AppearsOnTable, Expression, SelectableExpression, ValidGrouping, is_aggregate,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{BigInt, Integer};

use crate::backend::ClickHouse;

/// Generic wrapper for ClickHouse-specific `GROUP BY` modifiers.
#[derive(Debug, Clone, Copy)]
pub struct GroupByModifier<Expr> {
    expr: Expr,
    kind: GroupByModifierKind,
}

/// Which modifier syntax to render around or after the grouping expression.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum GroupByModifierKind {
    /// `GROUP BY expr WITH TOTALS`
    WithTotals,
    /// `GROUP BY ROLLUP(expr)`
    Rollup,
    /// `GROUP BY CUBE(expr)`
    Cube,
}

/// `GROUP BY ALL` marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct GroupByAll;

/// `GROUP BY GROUPING SETS ((...), (...), ...)` marker.
#[derive(Debug, Clone)]
pub struct GroupingSets {
    sets: Vec<Vec<String>>,
}

/// `GROUPING(expr, ...)` scalar function.
#[derive(Debug, Clone, Copy)]
pub struct Grouping<Expr> {
    expr: Expr,
}

/// Render `GROUP BY expr WITH TOTALS`.
pub fn with_totals<Expr>(expr: Expr) -> GroupByModifier<Expr>
where
    Expr: Expression,
{
    GroupByModifier {
        expr,
        kind: GroupByModifierKind::WithTotals,
    }
}

/// Render `GROUP BY ROLLUP(expr)`.
pub fn rollup<Expr>(expr: Expr) -> GroupByModifier<Expr>
where
    Expr: Expression,
{
    GroupByModifier {
        expr,
        kind: GroupByModifierKind::Rollup,
    }
}

/// Render `GROUP BY CUBE(expr)`.
pub fn cube<Expr>(expr: Expr) -> GroupByModifier<Expr>
where
    Expr: Expression,
{
    GroupByModifier {
        expr,
        kind: GroupByModifierKind::Cube,
    }
}

/// Render `GROUP BY ALL`.
pub fn group_by_all() -> GroupByAll {
    GroupByAll
}

/// Render `GROUP BY GROUPING SETS ((col_a), (col_b), ())`.
pub fn grouping_sets<I, Set, Name>(sets: I) -> GroupingSets
where
    I: IntoIterator<Item = Set>,
    Set: IntoIterator<Item = Name>,
    Name: Into<String>,
{
    GroupingSets {
        sets: sets
            .into_iter()
            .map(|set| set.into_iter().map(Into::into).collect())
            .collect(),
    }
}

/// Render `GROUPING(expr)`.
pub fn grouping<Expr>(expr: Expr) -> Grouping<Expr>
where
    Expr: Expression,
{
    Grouping { expr }
}

impl<Expr> Expression for GroupByModifier<Expr>
where
    Expr: Expression,
{
    type SqlType = Expr::SqlType;
}

impl Expression for GroupByAll {
    type SqlType = Integer;
}

impl Expression for GroupingSets {
    type SqlType = Integer;
}

impl<Expr> Expression for Grouping<Expr>
where
    Expr: Expression,
{
    type SqlType = BigInt;
}

impl<Expr, GB> ValidGrouping<GB> for GroupByModifier<Expr>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<GB> ValidGrouping<GB> for GroupByAll {
    type IsAggregate = is_aggregate::No;
}

impl<GB> ValidGrouping<GB> for GroupingSets {
    type IsAggregate = is_aggregate::No;
}

impl<Expr, GB> ValidGrouping<GB> for Grouping<Expr> {
    type IsAggregate = is_aggregate::No;
}

impl<Expr, QS> AppearsOnTable<QS> for GroupByModifier<Expr>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<QS> AppearsOnTable<QS> for GroupByAll where Self: Expression {}
impl<QS> AppearsOnTable<QS> for GroupingSets where Self: Expression {}
impl<Expr, QS> AppearsOnTable<QS> for Grouping<Expr> where Self: Expression {}

impl<Expr, QS> SelectableExpression<QS> for GroupByModifier<Expr> where Self: AppearsOnTable<QS> {}
impl<QS> SelectableExpression<QS> for GroupByAll where Self: AppearsOnTable<QS> {}
impl<QS> SelectableExpression<QS> for GroupingSets where Self: AppearsOnTable<QS> {}
impl<Expr, QS> SelectableExpression<QS> for Grouping<Expr> where Self: AppearsOnTable<QS> {}

impl<Expr> QueryId for GroupByModifier<Expr>
where
    Expr: QueryId,
{
    type QueryId = GroupByModifier<Expr::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl QueryId for GroupByAll {
    type QueryId = GroupByAll;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl QueryId for GroupingSets {
    type QueryId = GroupingSets;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr> QueryId for Grouping<Expr>
where
    Expr: QueryId,
{
    type QueryId = Grouping<Expr::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr, DB> QueryFragment<DB> for GroupByModifier<Expr>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        match self.kind {
            GroupByModifierKind::WithTotals => {
                self.expr.walk_ast(out.reborrow())?;
                out.push_sql(" WITH TOTALS");
            }
            GroupByModifierKind::Rollup => {
                out.push_sql("ROLLUP(");
                self.expr.walk_ast(out.reborrow())?;
                out.push_sql(")");
            }
            GroupByModifierKind::Cube => {
                out.push_sql("CUBE(");
                self.expr.walk_ast(out.reborrow())?;
                out.push_sql(")");
            }
        }
        Ok(())
    }
}

impl<DB> QueryFragment<DB> for GroupByAll
where
    DB: Backend,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql("ALL");
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for GroupingSets {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.sets.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse GROUPING SETS requires at least one set".into(),
            ));
        }

        out.push_sql("GROUPING SETS (");
        for (set_idx, set) in self.sets.iter().enumerate() {
            if set_idx > 0 {
                out.push_sql(", ");
            }
            out.push_sql("(");
            for (name_idx, name) in set.iter().enumerate() {
                if name_idx > 0 {
                    out.push_sql(", ");
                }
                push_qualified_identifier(&mut out, name)?;
            }
            out.push_sql(")");
        }
        out.push_sql(")");
        Ok(())
    }
}

impl<Expr, DB> QueryFragment<DB> for Grouping<Expr>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql("GROUPING(");
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

fn push_qualified_identifier(
    out: &mut AstPass<'_, '_, ClickHouse>,
    value: &str,
) -> QueryResult<()> {
    if value.trim().is_empty() {
        return Err(Error::QueryBuilderError(
            "empty ClickHouse identifier".into(),
        ));
    }

    for (idx, part) in value.split('.').enumerate() {
        validate_bare_identifier(part, "identifier")?;
        if idx > 0 {
            out.push_sql(".");
        }
        out.push_identifier(part)?;
    }
    Ok(())
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
