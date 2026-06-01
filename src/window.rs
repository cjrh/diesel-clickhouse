//! ClickHouse window functions, inline window specs, named windows, and QUALIFY.

use diesel::backend::Backend;
use diesel::expression::functions::define_sql_function;
use diesel::expression::{AppearsOnTable, Expression, SelectableExpression, ValidGrouping};
use diesel::query_builder::{AstPass, Query, QueryFragment, QueryId};
use diesel::query_dsl::RunQueryDsl;
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{BigInt, SingleValue, SqlType};

use crate::backend::ClickHouse;

// Window-capable functions. They are ordinary function-call AST nodes until a
// caller wraps them with [`OverDsl::over_ch`] or [`OverDsl::over_window`].
define_sql_function! {
    /// `row_number()`.
    #[sql_name = "row_number"]
    fn row_number() -> BigInt;
}

define_sql_function! {
    /// `rank()`.
    #[sql_name = "rank"]
    fn rank() -> BigInt;
}

define_sql_function! {
    /// `dense_rank()`.
    #[sql_name = "dense_rank"]
    fn dense_rank() -> BigInt;
}

define_sql_function! {
    /// `lag(expr)`.
    #[sql_name = "lag"]
    fn lag<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `lead(expr)`.
    #[sql_name = "lead"]
    fn lead<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `lagInFrame(expr, offset, default)`.
    #[sql_name = "lagInFrame"]
    fn lag_in_frame<T: SqlType + SingleValue>(expr: T, offset: BigInt, default: T) -> T;
}

define_sql_function! {
    /// `leadInFrame(expr, offset, default)`.
    #[sql_name = "leadInFrame"]
    fn lead_in_frame<T: SqlType + SingleValue>(expr: T, offset: BigInt, default: T) -> T;
}

define_sql_function! {
    /// `first_value(expr)`.
    #[sql_name = "first_value"]
    fn first_value<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `last_value(expr)`.
    #[sql_name = "last_value"]
    fn last_value<T: SqlType + SingleValue>(expr: T) -> T;
}

/// Build a window spec with `PARTITION BY expr`.
pub fn partition_by<Expr>(expr: Expr) -> WindowSpec<WindowPartition<Expr>>
where
    Expr: Expression,
{
    WindowSpec {
        partition: WindowPartition(expr),
        order: NoWindowOrder,
        frame: NoWindowFrame,
    }
}

/// Build a window spec with `ORDER BY expr` and no partition key.
pub fn window_order_by<Expr>(expr: Expr) -> WindowSpec<NoWindowPartition, WindowOrder<Expr>>
where
    Expr: Expression,
{
    WindowSpec {
        partition: NoWindowPartition,
        order: WindowOrder(expr),
        frame: NoWindowFrame,
    }
}

/// Append `QUALIFY predicate` to a query.
pub fn qualify<Q, Predicate>(query: Q, predicate: Predicate) -> QualifyQuery<Q, Predicate>
where
    Predicate: Expression,
{
    QualifyQuery { query, predicate }
}

/// Append `WINDOW name AS (spec)` to a query.
pub fn window<Q, Spec>(
    query: Q,
    name: impl Into<String>,
    spec: Spec,
) -> WindowQuery<Q, WindowBinding<NoWindowBindings, Spec>> {
    WindowQuery {
        query,
        bindings: WindowBinding {
            tail: NoWindowBindings,
            name: name.into(),
            spec,
        },
    }
}

/// `expr OVER (...)` wrapper.
#[derive(Debug, Clone, Copy)]
pub struct Over<Expr, Spec> {
    expr: Expr,
    spec: Spec,
}

/// `expr OVER window_name` wrapper.
#[derive(Debug, Clone)]
pub struct OverWindow<Expr> {
    expr: Expr,
    name: String,
}

/// Fluent `.over_ch(spec)` and `.over_window(name)` helpers for window functions.
pub trait OverDsl: Expression + Sized {
    /// Render `self OVER (spec)`.
    ///
    /// The `_ch` suffix avoids a name collision with Diesel 2.3's no-argument
    /// `.over()` helper while keeping ClickHouse window specifications fluent.
    fn over_ch<Spec>(self, spec: Spec) -> Over<Self, Spec> {
        Over { expr: self, spec }
    }

    /// Render `self OVER window_name`.
    fn over_window(self, name: impl Into<String>) -> OverWindow<Self> {
        OverWindow {
            expr: self,
            name: name.into(),
        }
    }
}

impl<T> OverDsl for T where T: Expression {}

/// Inline window specification.
#[derive(Debug, Clone, Copy)]
pub struct WindowSpec<Partition = NoWindowPartition, Order = NoWindowOrder, Frame = NoWindowFrame> {
    partition: Partition,
    order: Order,
    frame: Frame,
}

/// Missing `PARTITION BY` part.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWindowPartition;

/// `PARTITION BY expr` part.
#[derive(Debug, Clone, Copy)]
pub struct WindowPartition<Expr>(Expr);

/// Missing `ORDER BY` part.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWindowOrder;

/// `ORDER BY expr` part.
#[derive(Debug, Clone, Copy)]
pub struct WindowOrder<Expr>(Expr);

/// Missing frame clause.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWindowFrame;

/// Window frame units.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum WindowFrameUnits {
    /// `ROWS BETWEEN ...` counts physical rows relative to the current row.
    Rows,
    /// `RANGE BETWEEN ...` groups rows by distance in the ordering value.
    Range,
}

/// One boundary in a ClickHouse window frame.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum WindowFrameBound {
    /// `UNBOUNDED PRECEDING`.
    UnboundedPreceding,
    /// `n PRECEDING`.
    Preceding(u64),
    /// `CURRENT ROW`.
    CurrentRow,
    /// `n FOLLOWING`.
    Following(u64),
    /// `UNBOUNDED FOLLOWING`.
    UnboundedFollowing,
}

/// `ROWS` or `RANGE` frame clause for a window specification.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct WindowFrame {
    units: WindowFrameUnits,
    start: WindowFrameBound,
    end: WindowFrameBound,
}

/// Backwards-compatible name for the original frame helper return type.
pub type RowsBetweenUnboundedPrecedingAndCurrentRow = WindowFrame;

impl WindowFrameBound {
    /// Build `n PRECEDING`.
    pub fn preceding(offset: u64) -> Self {
        Self::Preceding(offset)
    }

    /// Build `n FOLLOWING`.
    pub fn following(offset: u64) -> Self {
        Self::Following(offset)
    }
}

impl<Partition, Order, Frame> WindowSpec<Partition, Order, Frame> {
    /// Add or replace the `ORDER BY` expression.
    pub fn order_by<Expr>(self, expr: Expr) -> WindowSpec<Partition, WindowOrder<Expr>, Frame>
    where
        Expr: Expression,
    {
        WindowSpec {
            partition: self.partition,
            order: WindowOrder(expr),
            frame: self.frame,
        }
    }

    /// Add a `ROWS BETWEEN start AND end` frame.
    pub fn rows_between(
        self,
        start: WindowFrameBound,
        end: WindowFrameBound,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.with_frame(WindowFrame {
            units: WindowFrameUnits::Rows,
            start,
            end,
        })
    }

    /// Add a `RANGE BETWEEN start AND end` frame.
    pub fn range_between(
        self,
        start: WindowFrameBound,
        end: WindowFrameBound,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.with_frame(WindowFrame {
            units: WindowFrameUnits::Range,
            start,
            end,
        })
    }

    /// Add `ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW`.
    pub fn rows_between_unbounded_preceding_and_current_row(
        self,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.rows_between(
            WindowFrameBound::UnboundedPreceding,
            WindowFrameBound::CurrentRow,
        )
    }

    /// Add `ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING`.
    pub fn rows_between_unbounded_preceding_and_unbounded_following(
        self,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.rows_between(
            WindowFrameBound::UnboundedPreceding,
            WindowFrameBound::UnboundedFollowing,
        )
    }

    /// Add `ROWS BETWEEN n PRECEDING AND CURRENT ROW`.
    pub fn rows_between_preceding_and_current_row(
        self,
        preceding: u64,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.rows_between(
            WindowFrameBound::Preceding(preceding),
            WindowFrameBound::CurrentRow,
        )
    }

    /// Add `ROWS BETWEEN n PRECEDING AND m FOLLOWING`.
    pub fn rows_between_preceding_and_following(
        self,
        preceding: u64,
        following: u64,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.rows_between(
            WindowFrameBound::Preceding(preceding),
            WindowFrameBound::Following(following),
        )
    }

    /// Add `ROWS BETWEEN CURRENT ROW AND n FOLLOWING`.
    pub fn rows_between_current_row_and_following(
        self,
        following: u64,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.rows_between(
            WindowFrameBound::CurrentRow,
            WindowFrameBound::Following(following),
        )
    }

    /// Add `RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW`.
    pub fn range_between_unbounded_preceding_and_current_row(
        self,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.range_between(
            WindowFrameBound::UnboundedPreceding,
            WindowFrameBound::CurrentRow,
        )
    }

    /// Add `RANGE BETWEEN n PRECEDING AND CURRENT ROW`.
    pub fn range_between_preceding_and_current_row(
        self,
        preceding: u64,
    ) -> WindowSpec<Partition, Order, WindowFrame> {
        self.range_between(
            WindowFrameBound::Preceding(preceding),
            WindowFrameBound::CurrentRow,
        )
    }

    fn with_frame<NewFrame>(self, frame: NewFrame) -> WindowSpec<Partition, Order, NewFrame> {
        WindowSpec {
            partition: self.partition,
            order: self.order,
            frame,
        }
    }
}

/// Query wrapper that appends `QUALIFY predicate`.
#[derive(Debug, Clone, Copy)]
pub struct QualifyQuery<Q, Predicate> {
    query: Q,
    predicate: Predicate,
}

/// Query wrapper that appends named window definitions.
#[derive(Debug, Clone, Copy)]
pub struct WindowQuery<Q, Bindings> {
    query: Q,
    bindings: Bindings,
}

/// Empty named window binding list.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWindowBindings;

/// One named window binding plus previously declared bindings.
#[derive(Debug, Clone)]
pub struct WindowBinding<Tail, Spec> {
    tail: Tail,
    name: String,
    spec: Spec,
}

impl<Q, Bindings> WindowQuery<Q, Bindings> {
    /// Add another named window definition.
    pub fn and_window<Spec>(
        self,
        name: impl Into<String>,
        spec: Spec,
    ) -> WindowQuery<Q, WindowBinding<Bindings, Spec>> {
        WindowQuery {
            query: self.query,
            bindings: WindowBinding {
                tail: self.bindings,
                name: name.into(),
                spec,
            },
        }
    }
}

impl<Expr, Spec> Expression for Over<Expr, Spec>
where
    Expr: Expression,
{
    type SqlType = Expr::SqlType;
}

impl<Expr> Expression for OverWindow<Expr>
where
    Expr: Expression,
{
    type SqlType = Expr::SqlType;
}

impl<Expr, Spec, GB> ValidGrouping<GB> for Over<Expr, Spec>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<Expr, GB> ValidGrouping<GB> for OverWindow<Expr>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<Expr, Spec, QS> AppearsOnTable<QS> for Over<Expr, Spec>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Expr, QS> AppearsOnTable<QS> for OverWindow<Expr>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Expr, Spec, QS> SelectableExpression<QS> for Over<Expr, Spec> where Self: AppearsOnTable<QS> {}
impl<Expr, QS> SelectableExpression<QS> for OverWindow<Expr> where Self: AppearsOnTable<QS> {}

impl<Expr, Spec> QueryId for Over<Expr, Spec>
where
    Expr: QueryId,
{
    type QueryId = Over<Expr::QueryId, ()>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr> QueryId for OverWindow<Expr>
where
    Expr: QueryId,
{
    type QueryId = OverWindow<Expr::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q, Predicate> Query for QualifyQuery<Q, Predicate>
where
    Q: Query,
{
    type SqlType = Q::SqlType;
}

impl<Q, Predicate, Conn> RunQueryDsl<Conn> for QualifyQuery<Q, Predicate> {}

impl<Q, Predicate> QueryId for QualifyQuery<Q, Predicate>
where
    Q: QueryId,
{
    type QueryId = QualifyQuery<Q::QueryId, ()>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q, Bindings> Query for WindowQuery<Q, Bindings>
where
    Q: Query,
{
    type SqlType = Q::SqlType;
}

impl<Q, Bindings, Conn> RunQueryDsl<Conn> for WindowQuery<Q, Bindings> {}

impl<Q, Bindings> QueryId for WindowQuery<Q, Bindings>
where
    Q: QueryId,
{
    type QueryId = WindowQuery<Q::QueryId, ()>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr, Spec, DB> QueryFragment<DB> for Over<Expr, Spec>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
    Spec: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(" OVER (");
        self.spec.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

impl<Expr> QueryFragment<ClickHouse> for OverWindow<Expr>
where
    Expr: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.expr.walk_ast(out.reborrow())?;
        validate_bare_identifier(&self.name, "window name")?;
        out.push_sql(" OVER ");
        out.push_identifier(&self.name)?;
        Ok(())
    }
}

trait WindowSpecPart {
    fn is_empty(&self) -> bool;
    fn walk_part<'b>(&'b self, out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()>;
}

impl WindowSpecPart for NoWindowPartition {
    fn is_empty(&self) -> bool {
        true
    }

    fn walk_part<'b>(&'b self, _out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Expr> WindowSpecPart for WindowPartition<Expr>
where
    Expr: QueryFragment<ClickHouse>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_part<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        out.push_sql("PARTITION BY ");
        self.0.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl WindowSpecPart for NoWindowOrder {
    fn is_empty(&self) -> bool {
        true
    }

    fn walk_part<'b>(&'b self, _out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Expr> WindowSpecPart for WindowOrder<Expr>
where
    Expr: QueryFragment<ClickHouse>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_part<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        out.push_sql("ORDER BY ");
        self.0.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl WindowSpecPart for NoWindowFrame {
    fn is_empty(&self) -> bool {
        true
    }

    fn walk_part<'b>(&'b self, _out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        Ok(())
    }
}

impl WindowSpecPart for WindowFrame {
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_part<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        out.push_sql(match self.units {
            WindowFrameUnits::Rows => "ROWS BETWEEN ",
            WindowFrameUnits::Range => "RANGE BETWEEN ",
        });
        push_frame_bound(&mut out, self.start);
        out.push_sql(" AND ");
        push_frame_bound(&mut out, self.end);
        Ok(())
    }
}

fn push_frame_bound<DB>(out: &mut AstPass<'_, '_, DB>, bound: WindowFrameBound)
where
    DB: Backend,
{
    match bound {
        WindowFrameBound::UnboundedPreceding => out.push_sql("UNBOUNDED PRECEDING"),
        WindowFrameBound::Preceding(offset) => {
            out.push_sql(&offset.to_string());
            out.push_sql(" PRECEDING");
        }
        WindowFrameBound::CurrentRow => out.push_sql("CURRENT ROW"),
        WindowFrameBound::Following(offset) => {
            out.push_sql(&offset.to_string());
            out.push_sql(" FOLLOWING");
        }
        WindowFrameBound::UnboundedFollowing => out.push_sql("UNBOUNDED FOLLOWING"),
    }
}

impl<Partition, Order, Frame> QueryFragment<ClickHouse> for WindowSpec<Partition, Order, Frame>
where
    Partition: WindowSpecPart,
    Order: WindowSpecPart,
    Frame: WindowSpecPart,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.partition.walk_part(out.reborrow())?;
        if !self.partition.is_empty() && !self.order.is_empty() {
            out.push_sql(" ");
        }
        self.order.walk_part(out.reborrow())?;
        if (!self.partition.is_empty() || !self.order.is_empty()) && !self.frame.is_empty() {
            out.push_sql(" ");
        }
        self.frame.walk_part(out.reborrow())?;
        Ok(())
    }
}

impl<Q, Predicate> QueryFragment<ClickHouse> for QualifyQuery<Q, Predicate>
where
    Q: QueryFragment<ClickHouse>,
    Predicate: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.query.walk_ast(out.reborrow())?;
        out.push_sql(" QUALIFY ");
        self.predicate.walk_ast(out.reborrow())?;
        Ok(())
    }
}

trait WindowBindings {
    fn is_empty(&self) -> bool;
    fn walk_bindings<'b>(&'b self, out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()>;
}

impl WindowBindings for NoWindowBindings {
    fn is_empty(&self) -> bool {
        true
    }

    fn walk_bindings<'b>(&'b self, _out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Tail, Spec> WindowBindings for WindowBinding<Tail, Spec>
where
    Tail: WindowBindings,
    Spec: QueryFragment<ClickHouse>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_bindings<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.tail.walk_bindings(out.reborrow())?;
        if !self.tail.is_empty() {
            out.push_sql(", ");
        }
        validate_bare_identifier(&self.name, "window name")?;
        out.push_identifier(&self.name)?;
        out.push_sql(" AS (");
        self.spec.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

impl<Q, Bindings> QueryFragment<ClickHouse> for WindowQuery<Q, Bindings>
where
    Q: QueryFragment<ClickHouse>,
    Bindings: WindowBindings,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.query.walk_ast(out.reborrow())?;
        if !self.bindings.is_empty() {
            out.push_sql(" WINDOW ");
            self.bindings.walk_bindings(out.reborrow())?;
        }
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
