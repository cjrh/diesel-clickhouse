//! ClickHouse-specific join sources.
//!
//! Diesel's built-in join nodes model ANSI `INNER` and `LEFT OUTER` joins.
//! ClickHouse extends the join grammar with `GLOBAL`, strictness modifiers
//! (`ANY`, `ALL`, `ASOF`), and result modifiers (`SEMI`, `ANTI`). This module
//! renders those forms as query sources so callers can keep composing with
//! Diesel `select`, `filter`, `order`, and trailing ClickHouse clauses. Diesel's
//! table macro does not know that columns are selectable from this custom join
//! source, so selecting from these joins currently uses explicit SQL select
//! expressions.

use diesel::expression::{
    AppearsOnTable, Expression, SelectableExpression, SqlLiteral, ValidGrouping,
    expression_types::Untyped, is_aggregate,
};
use diesel::query_builder::{AsQuery, AstPass, Query, QueryFragment, QueryId};
use diesel::query_dsl::{QueryDsl, RunQueryDsl, methods::SelectDsl};
use diesel::query_source::{AppearsInFromClause, Plus, QuerySource};
use diesel::result::{Error, QueryResult};

use crate::backend::ClickHouse;

/// Start building a ClickHouse-specific join source.
pub fn clickhouse_join<Left, Right>(left: Left, right: Right) -> ClickHouseJoinBuilder<Left, Right>
where
    Left: QuerySource,
    Right: QuerySource,
{
    ClickHouseJoinBuilder {
        left,
        right,
        global: false,
        strictness: None,
        kind: JoinKind::Inner,
        modifier: None,
    }
}

/// Fluent `.clickhouse_join(rhs)` entry point.
pub trait ClickHouseJoinDsl: Sized {
    /// Start building a ClickHouse-specific join between `self` and `right`.
    fn clickhouse_join<Right>(self, right: Right) -> ClickHouseJoinBuilder<Self, Right>
    where
        Self: QuerySource,
        Right: QuerySource,
    {
        clickhouse_join(self, right)
    }
}

impl<T> ClickHouseJoinDsl for T {}

/// Builder for ClickHouse's extended join syntax.
#[derive(Debug, Clone, Copy)]
pub struct ClickHouseJoinBuilder<Left: QuerySource, Right: QuerySource> {
    left: Left,
    right: Right,
    global: bool,
    strictness: Option<JoinStrictness>,
    kind: JoinKind,
    modifier: Option<JoinModifier>,
}

impl<Left, Right> ClickHouseJoinBuilder<Left, Right>
where
    Left: QuerySource,
    Right: QuerySource,
{
    /// Add `GLOBAL` before the join.
    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }

    /// Add the `ANY` strictness modifier.
    pub fn any(mut self) -> Self {
        self.strictness = Some(JoinStrictness::Any);
        self
    }

    /// Add the `ALL` strictness modifier.
    pub fn all(mut self) -> Self {
        self.strictness = Some(JoinStrictness::All);
        self
    }

    /// Add the `ASOF` strictness modifier.
    pub fn asof(mut self) -> Self {
        self.strictness = Some(JoinStrictness::Asof);
        self
    }

    /// Render an `INNER JOIN`.
    pub fn inner(mut self) -> Self {
        self.kind = JoinKind::Inner;
        self
    }

    /// Render a `LEFT JOIN`.
    pub fn left(mut self) -> Self {
        self.kind = JoinKind::Left;
        self
    }

    /// Render a `RIGHT JOIN`.
    pub fn right(mut self) -> Self {
        self.kind = JoinKind::Right;
        self
    }

    /// Render a `FULL JOIN`.
    pub fn full(mut self) -> Self {
        self.kind = JoinKind::Full;
        self
    }

    /// Render a `CROSS JOIN`.
    pub fn cross(mut self) -> Self {
        self.kind = JoinKind::Cross;
        self
    }

    /// Add the optional `OUTER` join modifier.
    pub fn outer(mut self) -> Self {
        self.modifier = Some(JoinModifier::Outer);
        self
    }

    /// Add the `SEMI` join modifier.
    pub fn semi(mut self) -> Self {
        self.modifier = Some(JoinModifier::Semi);
        self
    }

    /// Add the `ANTI` join modifier.
    pub fn anti(mut self) -> Self {
        self.modifier = Some(JoinModifier::Anti);
        self
    }

    /// Finish the join with an `ON predicate` clause.
    pub fn on<Predicate>(
        self,
        predicate: Predicate,
    ) -> ClickHouseJoin<Left, Right, JoinOn<Predicate>>
    where
        Predicate: Expression,
    {
        ClickHouseJoin::new(self, JoinOn(predicate))
    }

    /// Finish the join with a `USING (columns...)` clause.
    pub fn using<I, S>(self, columns: I) -> ClickHouseJoin<Left, Right, JoinUsing>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ClickHouseJoin::new(
            self,
            JoinUsing {
                columns: columns.into_iter().map(Into::into).collect(),
            },
        )
    }
}

/// ClickHouse join strictness modifier.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum JoinStrictness {
    Any,
    All,
    Asof,
}

/// ClickHouse join kind.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

/// Optional ClickHouse join result modifier.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum JoinModifier {
    Outer,
    Semi,
    Anti,
}

/// `ON predicate` join constraint.
#[derive(Debug, Clone, Copy)]
pub struct JoinOn<Predicate>(Predicate);

/// `USING (columns...)` join constraint.
#[derive(Debug, Clone)]
pub struct JoinUsing {
    columns: Vec<String>,
}

/// A ClickHouse-specific join source.
#[derive(Debug, Clone)]
pub struct ClickHouseJoin<Left: QuerySource, Right: QuerySource, Constraint> {
    left_from: Left::FromClause,
    right_from: Right::FromClause,
    global: bool,
    strictness: Option<JoinStrictness>,
    kind: JoinKind,
    modifier: Option<JoinModifier>,
    constraint: Constraint,
}

impl<Left, Right, Constraint> ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource,
    Right: QuerySource,
{
    fn new(builder: ClickHouseJoinBuilder<Left, Right>, constraint: Constraint) -> Self {
        let left_from = builder.left.from_clause();
        let right_from = builder.right.from_clause();
        Self {
            left_from,
            right_from,
            global: builder.global,
            strictness: builder.strictness,
            kind: builder.kind,
            modifier: builder.modifier,
            constraint,
        }
    }
}

impl<Left, Right, Constraint> QuerySource for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource + Clone,
    Right: QuerySource + Clone,
    Left::FromClause: Clone,
    Right::FromClause: Clone,
    Constraint: Clone,
{
    type FromClause = Self;
    type DefaultSelection = SqlLiteral<Untyped>;

    fn from_clause(&self) -> Self::FromClause {
        self.clone()
    }

    fn default_selection(&self) -> Self::DefaultSelection {
        diesel::dsl::sql("*")
    }
}

impl<Left, Right, Constraint, QS> AppearsInFromClause<QS>
    for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource + AppearsInFromClause<QS>,
    Right: QuerySource + AppearsInFromClause<QS>,
    Left::Count: Plus<Right::Count>,
    QS: QuerySource,
{
    // This keeps source wrappers composable, but custom ClickHouse joins still
    // use raw select expressions because Diesel table columns only implement
    // `SelectableExpression` for Diesel's built-in join node types.
    type Count = <Left::Count as Plus<Right::Count>>::Output;
}

type SimpleSelect<QS> =
    diesel::internal::table_macro::SelectStatement<diesel::internal::table_macro::FromClause<QS>>;

impl<Left, Right, Constraint> AsQuery for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource + Clone,
    Right: QuerySource + Clone,
    Left::FromClause: Clone,
    Right::FromClause: Clone,
    Constraint: Clone,
    Self: QuerySource,
    SimpleSelect<Self>: Query,
{
    type SqlType = <SimpleSelect<Self> as Query>::SqlType;
    type Query = SimpleSelect<Self>;

    fn as_query(self) -> Self::Query {
        diesel::internal::table_macro::SelectStatement::simple(self)
    }
}

impl<Left, Right, Constraint> QueryDsl for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource + Clone,
    Right: QuerySource + Clone,
    Left::FromClause: Clone,
    Right::FromClause: Clone,
    Constraint: Clone,
    Self: AsQuery,
{
}

impl<Left, Right, Constraint, Conn> RunQueryDsl<Conn> for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource + Clone,
    Right: QuerySource + Clone,
    Left::FromClause: Clone,
    Right::FromClause: Clone,
    Constraint: Clone,
    Self: AsQuery,
{
}

impl<Left, Right, Constraint, Selection> SelectDsl<Selection>
    for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource + Clone,
    Right: QuerySource + Clone,
    Left::FromClause: Clone,
    Right::FromClause: Clone,
    Constraint: Clone,
    Self: AsQuery,
    Selection: Expression,
    <Self as AsQuery>::Query: SelectDsl<Selection>,
{
    type Output = <<Self as AsQuery>::Query as SelectDsl<Selection>>::Output;

    fn select(self, selection: Selection) -> Self::Output {
        self.as_query().select(selection)
    }
}

impl<Left, Right, Constraint> QueryId for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource,
    Right: QuerySource,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Predicate> QueryFragment<ClickHouse> for JoinOn<Predicate>
where
    Predicate: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        out.push_sql(" ON ");
        self.0.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for JoinUsing {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.columns.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse JOIN USING requires at least one column".into(),
            ));
        }
        out.push_sql(" USING (");
        for (idx, column) in self.columns.iter().enumerate() {
            if idx > 0 {
                out.push_sql(", ");
            }
            validate_bare_identifier(column, "USING column")?;
            out.push_identifier(column)?;
        }
        out.push_sql(")");
        Ok(())
    }
}

impl<Left, Right, Constraint> QueryFragment<ClickHouse> for ClickHouseJoin<Left, Right, Constraint>
where
    Left: QuerySource,
    Right: QuerySource,
    Left::FromClause: QueryFragment<ClickHouse>,
    Right::FromClause: QueryFragment<ClickHouse>,
    Constraint: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.left_from.walk_ast(out.reborrow())?;
        if self.global {
            out.push_sql(" GLOBAL");
        }
        if let Some(strictness) = self.strictness {
            out.push_sql(strictness.as_sql());
        }
        out.push_sql(self.kind.as_sql());
        if let Some(modifier) = self.modifier {
            out.push_sql(modifier.as_sql());
        }
        out.push_sql(" JOIN ");
        self.right_from.walk_ast(out.reborrow())?;
        self.constraint.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl JoinStrictness {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Any => " ANY",
            Self::All => " ALL",
            Self::Asof => " ASOF",
        }
    }
}

impl JoinKind {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Inner => " INNER",
            Self::Left => " LEFT",
            Self::Right => " RIGHT",
            Self::Full => " FULL",
            Self::Cross => " CROSS",
        }
    }
}

impl JoinModifier {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Outer => " OUTER",
            Self::Semi => " SEMI",
            Self::Anti => " ANTI",
        }
    }
}

/// A Diesel table column made selectable from a custom ClickHouse query source.
///
/// Diesel's `table!` macro only implements `SelectableExpression` for a column
/// against the table itself and Diesel's own built-in join nodes, so a bare
/// `events::id` cannot be passed to `.select(...)` on custom sources such as
/// [`ClickHouseJoin`], [`Final`](crate::Final), [`Sample`](crate::Sample), or
/// [`Prewhere`](crate::Prewhere). Wrapping it with [`source_column`] keeps the
/// column's SQL type — so the Rust type the query loads is still checked — and
/// renders the same `` `table`.`column` `` SQL, while satisfying the selection
/// bound for custom source wrappers.
///
/// The wrapper deliberately does **not** verify that the column's table appears
/// in the source. That from-clause check is exactly what Diesel's built-in joins
/// provide and what Rust's orphan rule prevents a third-party backend from
/// reproducing for arbitrary foreign columns. Predicates can still use real,
/// type-checked columns when Diesel accepts the source; only the projection
/// trades the appearance check for type-correct loading.
#[derive(Debug, Clone, Copy)]
pub struct JoinColumn<C>(C);

/// A selectable custom-source column that renders `AS alias`.
#[derive(Debug, Clone)]
pub struct AliasedColumn<C> {
    column: C,
    alias: String,
}

/// Wrap a Diesel column so it can be selected from custom ClickHouse sources.
///
/// ```ignore
/// final_table(events::table)
///     .select(source_column(events::id))
///     .load::<i64>(&mut conn)
///     .await?;
/// ```
pub fn source_column<C>(column: C) -> JoinColumn<C>
where
    C: Expression,
{
    JoinColumn(column)
}

/// Wrap a Diesel column and render `AS alias` for result-shape validation.
///
/// This is useful when selecting a qualified column through a join but loading
/// into a struct field that expects the unqualified column name.
pub fn source_column_as<C>(column: C, alias: impl Into<String>) -> AliasedColumn<C>
where
    C: Expression,
{
    AliasedColumn {
        column,
        alias: alias.into(),
    }
}

/// Backwards-compatible name for [`source_column`].
///
/// Historically this helper was introduced for [`ClickHouseJoin`], but it is
/// equally useful for all custom ClickHouse source wrappers.
pub fn join_column<C>(column: C) -> JoinColumn<C>
where
    C: Expression,
{
    source_column(column)
}

impl<C> Expression for JoinColumn<C>
where
    C: Expression,
{
    type SqlType = C::SqlType;
}

// Selectable / appears-on-table for *any* query source: the wrapper opts out of
// the from-clause appearance check (see the type's docs), so these are blanket
// impls keyed only on the local `JoinColumn` type.
impl<C, QS> AppearsOnTable<QS> for JoinColumn<C> where C: Expression {}

impl<C, QS> SelectableExpression<QS> for JoinColumn<C> where C: Expression {}

// `Never` marks the projection as valid in both grouped and non-grouped
// queries, matching the wrapper's "trust the caller" stance.
impl<C, GroupBy> ValidGrouping<GroupBy> for JoinColumn<C> {
    type IsAggregate = is_aggregate::Never;
}

impl<C> QueryId for JoinColumn<C> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<C> QueryFragment<ClickHouse> for JoinColumn<C>
where
    C: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        // A Diesel column already renders fully table-qualified
        // (`` `table`.`column` ``), which is exactly what a custom-source
        // projection needs.
        self.0.walk_ast(out)
    }
}

impl<C> Expression for AliasedColumn<C>
where
    C: Expression,
{
    type SqlType = C::SqlType;
}

impl<C, QS> AppearsOnTable<QS> for AliasedColumn<C> where C: Expression {}

impl<C, QS> SelectableExpression<QS> for AliasedColumn<C> where C: Expression {}

impl<C, GroupBy> ValidGrouping<GroupBy> for AliasedColumn<C> {
    type IsAggregate = is_aggregate::Never;
}

impl<C> QueryId for AliasedColumn<C> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<C> QueryFragment<ClickHouse> for AliasedColumn<C>
where
    C: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.column.walk_ast(out.reborrow())?;
        out.push_sql(" AS ");
        validate_bare_identifier(&self.alias, "column alias")?;
        out.push_identifier(&self.alias)?;
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
