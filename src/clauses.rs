//! ClickHouse query fragments for syntax that sits outside ordinary scalar
//! expressions.

use diesel::expression::{AsExpression, Expression, SqlLiteral, expression_types::Untyped};
use diesel::query_builder::{AsQuery, AstPass, Query, QueryFragment, QueryId};
use diesel::query_dsl::{QueryDsl, RunQueryDsl, methods::SelectDsl};
use diesel::query_source::{AppearsInFromClause, QuerySource};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::Double;

use crate::backend::ClickHouse;

/// Wrap a table or subquery with ClickHouse's `FINAL` modifier.
pub fn final_table<T>(source: T) -> Final<T>
where
    T: QuerySource,
{
    let from_clause = source.from_clause();
    Final {
        _source: source,
        from_clause,
    }
}

/// Wrap a table or subquery with ClickHouse's `SAMPLE <ratio>` modifier.
pub fn sample<T, Ratio>(source: T, ratio: Ratio) -> Sample<T, Ratio::Expression>
where
    T: QuerySource,
    Ratio: AsExpression<Double>,
{
    let from_clause = source.from_clause();
    Sample {
        _source: source,
        from_clause,
        ratio: ratio.as_expression(),
        offset: NoSampleOffset,
    }
}

/// Wrap a table or subquery with `SAMPLE <ratio> OFFSET <offset>`.
pub fn sample_offset<T, Ratio, Offset>(
    source: T,
    ratio: Ratio,
    offset: Offset,
) -> Sample<T, Ratio::Expression, SampleOffset<Offset::Expression>>
where
    T: QuerySource,
    Ratio: AsExpression<Double>,
    Offset: AsExpression<Double>,
{
    let from_clause = source.from_clause();
    Sample {
        _source: source,
        from_clause,
        ratio: ratio.as_expression(),
        offset: SampleOffset(offset.as_expression()),
    }
}

/// Wrap a table or subquery with ClickHouse's `PREWHERE` clause.
pub fn prewhere<T, Predicate>(source: T, predicate: Predicate) -> Prewhere<T, Predicate>
where
    T: QuerySource,
    Predicate: Expression,
{
    let from_clause = source.from_clause();
    Prewhere {
        _source: source,
        from_clause,
        predicate,
    }
}

/// Wrap a table or subquery with ClickHouse's `ARRAY JOIN` clause.
pub fn array_join_clause<T, Expr>(source: T, expr: Expr) -> ArrayJoin<T, Expr>
where
    T: QuerySource,
    Expr: Expression,
{
    array_join(source, expr, ArrayJoinKind::Inner, None)
}

/// Wrap a table or subquery with `ARRAY JOIN expr AS alias`.
pub fn array_join_clause_as<T, Expr>(
    source: T,
    expr: Expr,
    alias: impl Into<String>,
) -> ArrayJoin<T, Expr>
where
    T: QuerySource,
    Expr: Expression,
{
    array_join(source, expr, ArrayJoinKind::Inner, Some(alias.into()))
}

/// Wrap a table or subquery with ClickHouse's `LEFT ARRAY JOIN` clause.
pub fn left_array_join_clause<T, Expr>(source: T, expr: Expr) -> ArrayJoin<T, Expr>
where
    T: QuerySource,
    Expr: Expression,
{
    array_join(source, expr, ArrayJoinKind::Left, None)
}

/// Wrap a table or subquery with `LEFT ARRAY JOIN expr AS alias`.
pub fn left_array_join_clause_as<T, Expr>(
    source: T,
    expr: Expr,
    alias: impl Into<String>,
) -> ArrayJoin<T, Expr>
where
    T: QuerySource,
    Expr: Expression,
{
    array_join(source, expr, ArrayJoinKind::Left, Some(alias.into()))
}

fn array_join<T, Expr>(
    source: T,
    expr: Expr,
    kind: ArrayJoinKind,
    alias: Option<String>,
) -> ArrayJoin<T, Expr>
where
    T: QuerySource,
    Expr: Expression,
{
    let from_clause = source.from_clause();
    ArrayJoin {
        _source: source,
        from_clause,
        expr,
        kind,
        alias,
    }
}

/// Append `FORMAT <format>` to a query.
pub fn format<Q>(query: Q, format: Format) -> FormattedQuery<Q> {
    FormattedQuery { query, format }
}

/// Append ClickHouse's `INTO OUTFILE file_name` clause to a query.
pub fn into_outfile<Q>(query: Q, file_name: impl Into<String>) -> IntoOutfileQuery<Q> {
    IntoOutfileQuery {
        query,
        file_name: file_name.into(),
        and_stdout: false,
        mode: None,
        compression: None,
        compression_level: None,
    }
}

/// Append `SETTINGS ...` to a query.
pub fn settings<Q, I>(query: Q, settings: I) -> SettingsQuery<Q>
where
    I: IntoIterator<Item = Setting>,
{
    SettingsQuery {
        query,
        settings: settings.into_iter().collect(),
    }
}

/// Append `LIMIT n BY column` to a query.
pub fn limit_by_col<Q>(query: Q, limit: i64, column: impl Into<String>) -> LimitBy<Q> {
    LimitBy {
        query,
        limit,
        offset: None,
        columns: vec![column.into()],
    }
}

/// Append `WITH TIES` after a ClickHouse `LIMIT` clause.
pub fn with_ties<Q>(query: Q) -> LimitWithTies<Q> {
    LimitWithTies { query }
}

/// Prepend a scalar `WITH expr AS alias` binding to a query.
pub fn with_alias<Q, Expr>(
    query: Q,
    expr: Expr,
    alias: impl Into<String>,
) -> WithQuery<Q, WithBinding<NoWithBindings, Expr>>
where
    Expr: Expression,
{
    WithQuery {
        query,
        bindings: WithBinding {
            tail: NoWithBindings,
            expr,
            alias: alias.into(),
        },
    }
}

/// Prepend a common table expression binding to a query.
pub fn with_cte<Q, Cte>(
    query: Q,
    alias: impl Into<String>,
    cte: Cte,
) -> WithQuery<Q, WithCteBinding<NoWithBindings, Cte>> {
    WithQuery {
        query,
        bindings: WithCteBinding {
            tail: NoWithBindings,
            alias: alias.into(),
            query: cte,
            materialized: false,
        },
    }
}

/// Prepend a materialized common table expression binding to a query.
pub fn with_materialized_cte<Q, Cte>(
    query: Q,
    alias: impl Into<String>,
    cte: Cte,
) -> WithQuery<Q, WithCteBinding<NoWithBindings, Cte>> {
    WithQuery {
        query,
        bindings: WithCteBinding {
            tail: NoWithBindings,
            alias: alias.into(),
            query: cte,
            materialized: true,
        },
    }
}

/// Extension methods for final ClickHouse query modifiers.
pub trait ClickHouseQueryDsl: Sized {
    /// Append `FORMAT <format>`.
    fn format(self, format: Format) -> FormattedQuery<Self> {
        crate::clauses::format(self, format)
    }

    /// Append `INTO OUTFILE file_name`.
    fn into_outfile(self, file_name: impl Into<String>) -> IntoOutfileQuery<Self> {
        crate::clauses::into_outfile(self, file_name)
    }

    /// Append `SETTINGS ...`.
    fn settings<I>(self, settings: I) -> SettingsQuery<Self>
    where
        I: IntoIterator<Item = Setting>,
    {
        crate::clauses::settings(self, settings)
    }

    /// Append `LIMIT n BY column`.
    fn limit_by_col(self, limit: i64, column: impl Into<String>) -> LimitBy<Self> {
        crate::clauses::limit_by_col(self, limit, column)
    }

    /// Append `LIMIT offset, n BY column`.
    fn limit_by_col_offset(
        self,
        offset: i64,
        limit: i64,
        column: impl Into<String>,
    ) -> LimitBy<Self> {
        LimitBy {
            query: self,
            limit,
            offset: Some(offset),
            columns: vec![column.into()],
        }
    }

    /// Append `LIMIT n BY col1, col2, ...`.
    fn limit_by_cols<I, S>(self, limit: i64, columns: I) -> LimitBy<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        LimitBy {
            query: self,
            limit,
            offset: None,
            columns: columns.into_iter().map(Into::into).collect(),
        }
    }

    /// Append `WITH TIES` after a ClickHouse `LIMIT` clause.
    fn with_ties(self) -> LimitWithTies<Self> {
        crate::clauses::with_ties(self)
    }

    /// Prepend a scalar `WITH expr AS alias` binding to this query.
    fn with_alias<Expr>(
        self,
        expr: Expr,
        alias: impl Into<String>,
    ) -> WithQuery<Self, WithBinding<NoWithBindings, Expr>>
    where
        Expr: Expression,
    {
        crate::clauses::with_alias(self, expr, alias)
    }

    /// Prepend a common table expression binding to this query.
    fn with_cte<Cte>(
        self,
        alias: impl Into<String>,
        cte: Cte,
    ) -> WithQuery<Self, WithCteBinding<NoWithBindings, Cte>> {
        crate::clauses::with_cte(self, alias, cte)
    }

    /// Prepend a materialized common table expression binding to this query.
    fn with_materialized_cte<Cte>(
        self,
        alias: impl Into<String>,
        cte: Cte,
    ) -> WithQuery<Self, WithCteBinding<NoWithBindings, Cte>> {
        crate::clauses::with_materialized_cte(self, alias, cte)
    }

    /// Append `WINDOW name AS (spec)`.
    fn window<Spec>(
        self,
        name: impl Into<String>,
        spec: Spec,
    ) -> crate::window::WindowQuery<
        Self,
        crate::window::WindowBinding<crate::window::NoWindowBindings, Spec>,
    > {
        crate::window::window(self, name, spec)
    }

    /// Append `QUALIFY predicate`.
    fn qualify<Predicate>(
        self,
        predicate: Predicate,
    ) -> crate::window::QualifyQuery<Self, Predicate>
    where
        Predicate: Expression,
    {
        crate::window::qualify(self, predicate)
    }

    /// Treat this query source as `source ARRAY JOIN expr`.
    fn array_join<Expr>(self, expr: Expr) -> ArrayJoin<Self, Expr>
    where
        Self: QuerySource,
        Expr: Expression,
    {
        crate::clauses::array_join_clause(self, expr)
    }

    /// Treat this query source as `source ARRAY JOIN expr AS alias`.
    fn array_join_as<Expr>(self, expr: Expr, alias: impl Into<String>) -> ArrayJoin<Self, Expr>
    where
        Self: QuerySource,
        Expr: Expression,
    {
        crate::clauses::array_join_clause_as(self, expr, alias)
    }

    /// Treat this query source as `source LEFT ARRAY JOIN expr`.
    fn left_array_join<Expr>(self, expr: Expr) -> ArrayJoin<Self, Expr>
    where
        Self: QuerySource,
        Expr: Expression,
    {
        crate::clauses::left_array_join_clause(self, expr)
    }

    /// Treat this query source as `source LEFT ARRAY JOIN expr AS alias`.
    fn left_array_join_as<Expr>(self, expr: Expr, alias: impl Into<String>) -> ArrayJoin<Self, Expr>
    where
        Self: QuerySource,
        Expr: Expression,
    {
        crate::clauses::left_array_join_clause_as(self, expr, alias)
    }
}

impl<T> ClickHouseQueryDsl for T {}

/// `FROM table FINAL` query source wrapper.
#[derive(Debug, Clone)]
pub struct Final<T: QuerySource> {
    _source: T,
    from_clause: T::FromClause,
}

/// `FROM table SAMPLE ratio [OFFSET offset]` query source wrapper.
#[derive(Debug, Clone)]
pub struct Sample<T: QuerySource, Ratio, Offset = NoSampleOffset> {
    _source: T,
    from_clause: T::FromClause,
    ratio: Ratio,
    offset: Offset,
}

/// No `SAMPLE OFFSET` clause.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSampleOffset;

/// `OFFSET expr` part of a ClickHouse `SAMPLE` clause.
#[derive(Debug, Clone, Copy)]
pub struct SampleOffset<Expr>(Expr);

/// `FROM table PREWHERE predicate` query source wrapper.
#[derive(Debug, Clone)]
pub struct Prewhere<T: QuerySource, Predicate> {
    _source: T,
    from_clause: T::FromClause,
    predicate: Predicate,
}

/// `FROM table ARRAY JOIN expr` query source wrapper.
#[derive(Debug, Clone)]
pub struct ArrayJoin<T: QuerySource, Expr> {
    _source: T,
    from_clause: T::FromClause,
    expr: Expr,
    kind: ArrayJoinKind,
    alias: Option<String>,
}

/// Which ClickHouse ARRAY JOIN form to render.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum ArrayJoinKind {
    /// `ARRAY JOIN`
    Inner,
    /// `LEFT ARRAY JOIN`
    Left,
}

/// Query wrapper that appends a ClickHouse `FORMAT` clause.
#[derive(Debug, Clone, Copy)]
pub struct FormattedQuery<Q> {
    query: Q,
    format: Format,
}

/// Query wrapper that appends ClickHouse's `INTO OUTFILE` clause.
#[derive(Debug, Clone)]
pub struct IntoOutfileQuery<Q> {
    query: Q,
    file_name: String,
    and_stdout: bool,
    mode: Option<OutfileMode>,
    compression: Option<OutfileCompression>,
    compression_level: Option<u8>,
}

/// Query wrapper that appends a ClickHouse `SETTINGS` clause.
#[derive(Debug, Clone)]
pub struct SettingsQuery<Q> {
    query: Q,
    settings: Vec<Setting>,
}

/// Query wrapper that appends `LIMIT ... BY ...`.
#[derive(Debug, Clone)]
pub struct LimitBy<Q> {
    query: Q,
    limit: i64,
    offset: Option<i64>,
    columns: Vec<String>,
}

/// Query wrapper that appends `WITH TIES` after `LIMIT`.
#[derive(Debug, Clone, Copy)]
pub struct LimitWithTies<Q> {
    query: Q,
}

/// Query wrapper that prepends ClickHouse scalar `WITH` aliases.
#[derive(Debug, Clone, Copy)]
pub struct WithQuery<Q, Bindings> {
    query: Q,
    bindings: Bindings,
}

/// Empty scalar `WITH` binding list.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWithBindings;

/// One scalar `WITH expr AS alias` binding plus previously declared bindings.
#[derive(Debug, Clone)]
pub struct WithBinding<Tail, Expr> {
    tail: Tail,
    expr: Expr,
    alias: String,
}

/// One `WITH alias AS [MATERIALIZED] (subquery)` binding.
#[derive(Debug, Clone)]
pub struct WithCteBinding<Tail, Cte> {
    tail: Tail,
    alias: String,
    query: Cte,
    materialized: bool,
}

impl<Q> IntoOutfileQuery<Q> {
    /// Also write the exported rows to stdout.
    pub fn and_stdout(mut self) -> Self {
        self.and_stdout = true;
        self
    }

    /// Append to an existing file.
    ///
    /// ClickHouse does not allow `APPEND` together with `COMPRESSION`; this is
    /// validated during SQL rendering.
    pub fn append(mut self) -> Self {
        self.mode = Some(OutfileMode::Append);
        self
    }

    /// Truncate an existing file before writing.
    pub fn truncate(mut self) -> Self {
        self.mode = Some(OutfileMode::Truncate);
        self
    }

    /// Add an explicit `COMPRESSION` clause.
    pub fn compression(mut self, compression: OutfileCompression) -> Self {
        self.compression = Some(compression);
        self
    }

    /// Add `COMPRESSION type LEVEL level`.
    pub fn compression_with_level(mut self, compression: OutfileCompression, level: u8) -> Self {
        self.compression = Some(compression);
        self.compression_level = Some(level);
        self
    }
}

impl<Q, Bindings> WithQuery<Q, Bindings> {
    /// Add another scalar `WITH expr AS alias` binding.
    pub fn and_with_alias<Expr>(
        self,
        expr: Expr,
        alias: impl Into<String>,
    ) -> WithQuery<Q, WithBinding<Bindings, Expr>>
    where
        Expr: Expression,
    {
        WithQuery {
            query: self.query,
            bindings: WithBinding {
                tail: self.bindings,
                expr,
                alias: alias.into(),
            },
        }
    }

    /// Add another common table expression binding.
    pub fn and_with_cte<Cte>(
        self,
        alias: impl Into<String>,
        cte: Cte,
    ) -> WithQuery<Q, WithCteBinding<Bindings, Cte>> {
        WithQuery {
            query: self.query,
            bindings: WithCteBinding {
                tail: self.bindings,
                alias: alias.into(),
                query: cte,
                materialized: false,
            },
        }
    }

    /// Add another materialized common table expression binding.
    pub fn and_with_materialized_cte<Cte>(
        self,
        alias: impl Into<String>,
        cte: Cte,
    ) -> WithQuery<Q, WithCteBinding<Bindings, Cte>> {
        WithQuery {
            query: self.query,
            bindings: WithCteBinding {
                tail: self.bindings,
                alias: alias.into(),
                query: cte,
                materialized: true,
            },
        }
    }
}

/// Supported ClickHouse output formats.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Format {
    TabSeparated,
    Csv,
    Json,
    JsonEachRow,
    Native,
    RowBinary,
    Parquet,
    Arrow,
    Null,
    /// A caller-provided format name. It must be a bare identifier.
    Custom(&'static str),
}

impl Format {
    fn as_sql(self) -> &'static str {
        match self {
            Self::TabSeparated => "TabSeparated",
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::JsonEachRow => "JSONEachRow",
            Self::Native => "Native",
            Self::RowBinary => "RowBinary",
            Self::Parquet => "Parquet",
            Self::Arrow => "Arrow",
            Self::Null => "Null",
            Self::Custom(name) => name,
        }
    }
}

/// Existing-file behavior for `INTO OUTFILE`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum OutfileMode {
    Append,
    Truncate,
}

/// Compression algorithms supported by ClickHouse `INTO OUTFILE`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum OutfileCompression {
    None,
    Gzip,
    Deflate,
    Brotli,
    Xz,
    Zstd,
    Lz4,
    Bz2,
}

impl OutfileCompression {
    fn as_sql(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
            Self::Brotli => "br",
            Self::Xz => "xz",
            Self::Zstd => "zstd",
            Self::Lz4 => "lz4",
            Self::Bz2 => "bz2",
        }
    }

    fn max_level(self) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Lz4 => Some(12),
            Self::Zstd => Some(22),
            Self::Gzip | Self::Deflate | Self::Brotli | Self::Xz | Self::Bz2 => Some(9),
        }
    }
}

/// One `SETTINGS name = value` entry. Use [`Setting::flag`] for ClickHouse's
/// boolean shorthand (`SETTINGS some_flag`).
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    name: String,
    value: Option<SettingValue>,
}

impl Setting {
    pub fn new(name: impl Into<String>, value: impl Into<SettingValue>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
        }
    }

    pub fn flag(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: None,
        }
    }
}

/// Literal value in a ClickHouse `SETTINGS` clause.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
}

impl From<bool> for SettingValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<i64> for SettingValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
impl From<i32> for SettingValue {
    fn from(value: i32) -> Self {
        Self::Int(value.into())
    }
}
impl From<u64> for SettingValue {
    fn from(value: u64) -> Self {
        Self::UInt(value)
    }
}
impl From<u32> for SettingValue {
    fn from(value: u32) -> Self {
        Self::UInt(value.into())
    }
}
impl From<f64> for SettingValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}
impl From<f32> for SettingValue {
    fn from(value: f32) -> Self {
        Self::Float(value.into())
    }
}
impl From<String> for SettingValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<&str> for SettingValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl<T> QuerySource for Final<T>
where
    T: QuerySource + Clone,
    T::FromClause: Clone,
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

impl<T, Ratio, Offset> QuerySource for Sample<T, Ratio, Offset>
where
    T: QuerySource + Clone,
    T::FromClause: Clone,
    Ratio: Expression + Clone,
    Offset: Clone,
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

impl<T, Predicate> QuerySource for Prewhere<T, Predicate>
where
    T: QuerySource + Clone,
    T::FromClause: Clone,
    Predicate: Expression + Clone,
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

impl<T, Expr> QuerySource for ArrayJoin<T, Expr>
where
    T: QuerySource + Clone,
    T::FromClause: Clone,
    Expr: Expression + Clone,
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

impl<T, QS> AppearsInFromClause<QS> for Final<T>
where
    T: QuerySource + AppearsInFromClause<QS>,
    QS: QuerySource,
{
    type Count = <T as AppearsInFromClause<QS>>::Count;
}

impl<T, Ratio, Offset, QS> AppearsInFromClause<QS> for Sample<T, Ratio, Offset>
where
    T: QuerySource + AppearsInFromClause<QS>,
    Ratio: Expression,
    QS: QuerySource,
{
    type Count = <T as AppearsInFromClause<QS>>::Count;
}

impl<T, Predicate, QS> AppearsInFromClause<QS> for Prewhere<T, Predicate>
where
    T: QuerySource + AppearsInFromClause<QS>,
    Predicate: Expression,
    QS: QuerySource,
{
    type Count = <T as AppearsInFromClause<QS>>::Count;
}

impl<T, Expr, QS> AppearsInFromClause<QS> for ArrayJoin<T, Expr>
where
    T: QuerySource + AppearsInFromClause<QS>,
    Expr: Expression,
    QS: QuerySource,
{
    type Count = <T as AppearsInFromClause<QS>>::Count;
}

type SimpleSelect<QS> =
    diesel::internal::table_macro::SelectStatement<diesel::internal::table_macro::FromClause<QS>>;

macro_rules! impl_query_source_query_dsl {
    ($wrapper:ident <$($generic:ident),+> where $($bounds:tt)+) => {
        impl<$($generic),+> AsQuery for $wrapper<$($generic),+>
        where
            $($bounds)+
            Self: QuerySource,
            SimpleSelect<Self>: Query,
        {
            type SqlType = <SimpleSelect<Self> as Query>::SqlType;
            type Query = SimpleSelect<Self>;

            fn as_query(self) -> Self::Query {
                diesel::internal::table_macro::SelectStatement::simple(self)
            }
        }

        impl<$($generic),+> QueryDsl for $wrapper<$($generic),+>
        where
            $($bounds)+
            Self: AsQuery,
        {
        }

        impl<$($generic),+, Conn> RunQueryDsl<Conn> for $wrapper<$($generic),+>
        where
            $($bounds)+
            Self: AsQuery,
        {
        }

        impl<$($generic),+, Selection> SelectDsl<Selection> for $wrapper<$($generic),+>
        where
            $($bounds)+
            Self: AsQuery,
            Selection: Expression,
            <Self as AsQuery>::Query: SelectDsl<Selection>,
        {
            type Output = <<Self as AsQuery>::Query as SelectDsl<Selection>>::Output;

            fn select(self, selection: Selection) -> Self::Output {
                self.as_query().select(selection)
            }
        }
    };
}

impl_query_source_query_dsl!(Final<T> where T: QuerySource + Clone, T::FromClause: Clone,);
impl_query_source_query_dsl!(Sample<T, Ratio, Offset> where T: QuerySource + Clone, T::FromClause: Clone, Ratio: Expression + Clone, Offset: Clone,);
impl_query_source_query_dsl!(Prewhere<T, Predicate> where T: QuerySource + Clone, T::FromClause: Clone, Predicate: Expression + Clone,);
impl_query_source_query_dsl!(ArrayJoin<T, Expr> where T: QuerySource + Clone, T::FromClause: Clone, Expr: Expression + Clone,);

impl<T> QueryId for Final<T>
where
    T: QuerySource,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<T, Ratio, Offset> QueryId for Sample<T, Ratio, Offset>
where
    T: QuerySource,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<T, Predicate> QueryId for Prewhere<T, Predicate>
where
    T: QuerySource,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<T, Expr> QueryId for ArrayJoin<T, Expr>
where
    T: QuerySource,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<T> QueryFragment<ClickHouse> for Final<T>
where
    T: QuerySource,
    T::FromClause: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.from_clause.walk_ast(out.reborrow())?;
        out.push_sql(" FINAL");
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for NoSampleOffset {
    fn walk_ast<'b>(&'b self, _out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Expr> QueryFragment<ClickHouse> for SampleOffset<Expr>
where
    Expr: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        out.push_sql(" OFFSET ");
        self.0.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl<T, Ratio, Offset> QueryFragment<ClickHouse> for Sample<T, Ratio, Offset>
where
    T: QuerySource,
    T::FromClause: QueryFragment<ClickHouse>,
    Ratio: QueryFragment<ClickHouse>,
    Offset: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.from_clause.walk_ast(out.reborrow())?;
        out.push_sql(" SAMPLE ");
        self.ratio.walk_ast(out.reborrow())?;
        self.offset.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl<T, Predicate> QueryFragment<ClickHouse> for Prewhere<T, Predicate>
where
    T: QuerySource,
    T::FromClause: QueryFragment<ClickHouse>,
    Predicate: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.from_clause.walk_ast(out.reborrow())?;
        out.push_sql(" PREWHERE ");
        self.predicate.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl<T, Expr> QueryFragment<ClickHouse> for ArrayJoin<T, Expr>
where
    T: QuerySource,
    T::FromClause: QueryFragment<ClickHouse>,
    Expr: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.from_clause.walk_ast(out.reborrow())?;
        match self.kind {
            ArrayJoinKind::Inner => out.push_sql(" ARRAY JOIN "),
            ArrayJoinKind::Left => out.push_sql(" LEFT ARRAY JOIN "),
        }
        self.expr.walk_ast(out.reborrow())?;
        if let Some(alias) = &self.alias {
            validate_bare_identifier(alias, "ARRAY JOIN alias")?;
            out.push_sql(" AS ");
            out.push_identifier(alias)?;
        }
        Ok(())
    }
}

macro_rules! impl_query_wrapper_traits {
    ($wrapper:ident <$($generic:ident),+>) => {
        impl<$($generic),+> Query for $wrapper<$($generic),+>
        where
            Q: Query,
        {
            type SqlType = Q::SqlType;
        }

        impl<$($generic),+, Conn> RunQueryDsl<Conn> for $wrapper<$($generic),+> {}
    };
}

impl_query_wrapper_traits!(FormattedQuery<Q>);
impl_query_wrapper_traits!(IntoOutfileQuery<Q>);
impl_query_wrapper_traits!(SettingsQuery<Q>);
impl_query_wrapper_traits!(LimitBy<Q>);
impl_query_wrapper_traits!(LimitWithTies<Q>);
impl_query_wrapper_traits!(WithQuery<Q, Bindings>);

impl<Q> QueryId for FormattedQuery<Q>
where
    Q: QueryId,
{
    type QueryId = FormattedQuery<Q::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q> QueryId for IntoOutfileQuery<Q>
where
    Q: QueryId,
{
    type QueryId = IntoOutfileQuery<Q::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q> QueryId for SettingsQuery<Q>
where
    Q: QueryId,
{
    type QueryId = SettingsQuery<Q::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q> QueryId for LimitBy<Q>
where
    Q: QueryId,
{
    type QueryId = LimitBy<Q::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q> QueryId for LimitWithTies<Q>
where
    Q: QueryId,
{
    type QueryId = LimitWithTies<Q::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q, Bindings> QueryId for WithQuery<Q, Bindings>
where
    Q: QueryId,
{
    type QueryId = WithQuery<Q::QueryId, ()>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Q> QueryFragment<ClickHouse> for FormattedQuery<Q>
where
    Q: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.query.walk_ast(out.reborrow())?;
        let format = self.format.as_sql();
        validate_bare_identifier(format, "FORMAT")?;
        out.push_sql(" FORMAT ");
        out.push_sql(format);
        Ok(())
    }
}

impl<Q> QueryFragment<ClickHouse> for IntoOutfileQuery<Q>
where
    Q: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.file_name.trim().is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse INTO OUTFILE file name must not be empty".into(),
            ));
        }
        if self.mode == Some(OutfileMode::Append) && self.compression.is_some() {
            return Err(Error::QueryBuilderError(
                "ClickHouse INTO OUTFILE APPEND cannot be used with COMPRESSION".into(),
            ));
        }
        let compression = self.compression;
        if let Some(level) = self.compression_level {
            let Some(compression) = compression else {
                return Err(Error::QueryBuilderError(
                    "ClickHouse INTO OUTFILE LEVEL requires COMPRESSION".into(),
                ));
            };
            let Some(max_level) = compression.max_level() else {
                return Err(Error::QueryBuilderError(
                    "ClickHouse INTO OUTFILE COMPRESSION 'none' does not support LEVEL".into(),
                ));
            };
            if level == 0 || level > max_level {
                return Err(Error::QueryBuilderError(
                    format!(
                        "ClickHouse INTO OUTFILE COMPRESSION '{}' LEVEL must be between 1 and {max_level}, got {level}",
                        compression.as_sql()
                    )
                    .into(),
                ));
            }
        }

        self.query.walk_ast(out.reborrow())?;
        out.push_sql(" INTO OUTFILE ");
        push_string_literal(&mut out, &self.file_name);
        if self.and_stdout {
            out.push_sql(" AND STDOUT");
        }
        match self.mode {
            Some(OutfileMode::Append) => out.push_sql(" APPEND"),
            Some(OutfileMode::Truncate) => out.push_sql(" TRUNCATE"),
            None => {}
        }
        if let Some(compression) = compression {
            out.push_sql(" COMPRESSION ");
            push_string_literal(&mut out, compression.as_sql());
            if let Some(level) = self.compression_level {
                out.push_sql(" LEVEL ");
                out.push_sql(&level.to_string());
            }
        }
        Ok(())
    }
}

impl<Q> QueryFragment<ClickHouse> for SettingsQuery<Q>
where
    Q: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.query.walk_ast(out.reborrow())?;
        if self.settings.is_empty() {
            return Ok(());
        }
        out.push_sql(" SETTINGS ");
        for (idx, setting) in self.settings.iter().enumerate() {
            if idx > 0 {
                out.push_sql(", ");
            }
            validate_bare_identifier(&setting.name, "setting")?;
            out.push_sql(&setting.name);
            if let Some(value) = &setting.value {
                out.push_sql(" = ");
                push_setting_value(&mut out, value)?;
            }
        }
        Ok(())
    }
}

impl<Q> QueryFragment<ClickHouse> for LimitBy<Q>
where
    Q: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.columns.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse LIMIT BY requires at least one column".into(),
            ));
        }

        self.query.walk_ast(out.reborrow())?;
        out.push_sql(" LIMIT ");
        if let Some(offset) = self.offset {
            out.push_sql(&offset.to_string());
            out.push_sql(", ");
        }
        out.push_sql(&self.limit.to_string());
        out.push_sql(" BY ");
        for (idx, column) in self.columns.iter().enumerate() {
            if idx > 0 {
                out.push_sql(", ");
            }
            push_qualified_identifier(&mut out, column)?;
        }
        Ok(())
    }
}

impl<Q> QueryFragment<ClickHouse> for LimitWithTies<Q>
where
    Q: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.query.walk_ast(out.reborrow())?;
        out.push_sql(" WITH TIES");
        Ok(())
    }
}

trait WithBindings {
    fn is_empty(&self) -> bool;
    fn walk_bindings<'b>(&'b self, out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()>;
}

impl WithBindings for NoWithBindings {
    fn is_empty(&self) -> bool {
        true
    }

    fn walk_bindings<'b>(&'b self, _out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        Ok(())
    }
}

impl<Tail, Expr> WithBindings for WithBinding<Tail, Expr>
where
    Tail: WithBindings,
    Expr: QueryFragment<ClickHouse>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_bindings<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.tail.walk_bindings(out.reborrow())?;
        if !self.tail.is_empty() {
            out.push_sql(", ");
        }
        self.expr.walk_ast(out.reborrow())?;
        out.push_sql(" AS ");
        validate_bare_identifier(&self.alias, "WITH alias")?;
        out.push_identifier(&self.alias)?;
        Ok(())
    }
}

impl<Tail, Cte> WithBindings for WithCteBinding<Tail, Cte>
where
    Tail: WithBindings,
    Cte: QueryFragment<ClickHouse>,
{
    fn is_empty(&self) -> bool {
        false
    }

    fn walk_bindings<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.tail.walk_bindings(out.reborrow())?;
        if !self.tail.is_empty() {
            out.push_sql(", ");
        }
        validate_bare_identifier(&self.alias, "CTE alias")?;
        out.push_identifier(&self.alias)?;
        out.push_sql(" AS");
        if self.materialized {
            out.push_sql(" MATERIALIZED");
        }
        out.push_sql(" (");
        self.query.walk_ast(out.reborrow())?;
        out.push_sql(")");
        Ok(())
    }
}

impl<Q, Bindings> QueryFragment<ClickHouse> for WithQuery<Q, Bindings>
where
    Q: QueryFragment<ClickHouse>,
    Bindings: WithBindings,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if !self.bindings.is_empty() {
            out.push_sql("WITH ");
            self.bindings.walk_bindings(out.reborrow())?;
            out.push_sql(" ");
        }
        self.query.walk_ast(out.reborrow())?;
        Ok(())
    }
}

fn push_setting_value(
    out: &mut AstPass<'_, '_, ClickHouse>,
    value: &SettingValue,
) -> QueryResult<()> {
    match value {
        SettingValue::Bool(value) => out.push_sql(if *value { "1" } else { "0" }),
        SettingValue::Int(value) => out.push_sql(&value.to_string()),
        SettingValue::UInt(value) => out.push_sql(&value.to_string()),
        SettingValue::Float(value) => {
            if !value.is_finite() {
                return Err(Error::QueryBuilderError(
                    format!("ClickHouse setting value must be finite, got {value}").into(),
                ));
            }
            out.push_sql(&value.to_string());
        }
        SettingValue::String(value) => push_string_literal(out, value),
    }
    Ok(())
}

fn push_string_literal(out: &mut AstPass<'_, '_, ClickHouse>, value: &str) {
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
