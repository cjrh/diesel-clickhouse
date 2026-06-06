//! Lightweight ClickHouse backend for Diesel SQL rendering.
//!
//! The backend gives Diesel a ClickHouse-shaped SQL dialect (`backtick`
//! identifiers, `?` placeholders and ANSI-ish `SELECT` layout).  It is shared
//! by the render-only [`to_sql`] helper and the HTTP-backed
//! [`AsyncClickHouseConnection`](crate::AsyncClickHouseConnection).

use std::borrow::Cow;

use diesel::backend::{
    Backend, DieselReserveSpecialization, SqlDialect, TrustedBackend, sql_dialect,
};
use diesel::query_builder::{
    AstPass, BoxedLimitOffsetClause, LimitOffsetClause, QueryBuilder, QueryFragment,
    bind_collector::RawBytesBindCollector,
};
use diesel::result::QueryResult;
use diesel::sql_types::{
    BigInt, Binary, Bool, Date, Double, Float, HasSqlType, Integer, Numeric, SmallInt, Text, Time,
    Timestamp, TypeMetadata,
};

/// Diesel backend marker for ClickHouse SQL rendering.
#[derive(Debug, Clone, Copy, Default, Hash, PartialEq, Eq)]
pub struct ClickHouse;

/// Minimal type metadata used by the bind collector.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ClickHouseTypeMetadata {
    /// Canonical ClickHouse type name for diagnostics and broad behavior groups.
    pub name: &'static str,
    parameter_type: Cow<'static, str>,
    bind_kind: ClickHouseBindKind,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum ClickHouseBindKind {
    Positional,
    NamedParameter,
}

impl ClickHouseTypeMetadata {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            parameter_type: Cow::Borrowed(name),
            bind_kind: ClickHouseBindKind::Positional,
        }
    }

    /// Build metadata where the server-side parameter type needs more detail
    /// than the broad canonical family name, such as `Decimal64(2)`.
    pub fn with_parameter_type(
        name: &'static str,
        parameter_type: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            name,
            parameter_type: parameter_type.into(),
            bind_kind: ClickHouseBindKind::Positional,
        }
    }

    /// Mark this metadata as a named ClickHouse HTTP parameter binding.
    pub(crate) fn into_named_parameter(mut self) -> Self {
        self.bind_kind = ClickHouseBindKind::NamedParameter;
        self
    }

    /// True when the bind bytes carry a named HTTP parameter value rather than
    /// a positional SQL placeholder value.
    pub(crate) fn is_named_parameter(&self) -> bool {
        self.bind_kind == ClickHouseBindKind::NamedParameter
    }

    /// ClickHouse type string used for HTTP server-side parameters.
    pub fn parameter_type(&self) -> &str {
        &self.parameter_type
    }
}

/// Query builder that renders ClickHouse-style SQL.
#[derive(Debug, Default, Clone)]
pub struct ClickHouseQueryBuilder {
    sql: String,
}

impl ClickHouseQueryBuilder {
    /// Constructs an empty query builder.
    pub fn new() -> Self {
        Self::default()
    }
}

impl QueryBuilder<ClickHouse> for ClickHouseQueryBuilder {
    fn push_sql(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    fn push_identifier(&mut self, identifier: &str) -> QueryResult<()> {
        self.sql.push('`');
        self.sql.push_str(&identifier.replace('`', "``"));
        self.sql.push('`');
        Ok(())
    }

    fn push_bind_param(&mut self) {
        self.push_bind_param_value_only();
        self.sql.push('?');
    }

    fn finish(self) -> String {
        self.sql
    }
}

impl TypeMetadata for ClickHouse {
    type TypeMetadata = ClickHouseTypeMetadata;
    type MetadataLookup = ();
}

macro_rules! has_sql_type {
    ($sql_type:ty => $clickhouse_name:literal) => {
        impl HasSqlType<$sql_type> for ClickHouse {
            fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
                ClickHouseTypeMetadata::new($clickhouse_name)
            }
        }
    };
}

has_sql_type!(Bool => "Bool");
has_sql_type!(SmallInt => "Int16");
has_sql_type!(Integer => "Int32");
has_sql_type!(BigInt => "Int64");
has_sql_type!(Float => "Float32");
has_sql_type!(Double => "Float64");
has_sql_type!(Text => "String");
has_sql_type!(Binary => "String");
has_sql_type!(Date => "Date");
has_sql_type!(Time => "DateTime");
has_sql_type!(Timestamp => "DateTime");
has_sql_type!(Numeric => "Decimal");

impl Backend for ClickHouse {
    type QueryBuilder = ClickHouseQueryBuilder;
    type RawValue<'a> = &'a [u8];
    type BindCollector<'a> = RawBytesBindCollector<ClickHouse>;
}

impl SqlDialect for ClickHouse {
    type ReturningClause = sql_dialect::returning_clause::DoesNotSupportReturningClause;
    type OnConflictClause = sql_dialect::on_conflict_clause::DoesNotSupportOnConflictClause;
    type InsertWithDefaultKeyword =
        sql_dialect::default_keyword_for_insert::DoesNotSupportDefaultKeyword;
    // Diesel's multi-row `BatchInsert` (`INSERT ... VALUES (..),(..)`) is
    // hardwired to require `IsoSqlDefaultKeyword`; backends without it (SQLite,
    // ClickHouse) can only batch through a backend-specific `QueryFragment`
    // impl, which Rust's orphan rule forbids a third-party backend from writing.
    // We therefore declare no single-query batch support: single-row inserts go
    // through Diesel, and high-throughput multi-row ingestion uses
    // `AsyncClickHouseConnection::insert_batch`, which drives the `clickhouse`
    // client's RowBinary inserter (see `docs/USAGE.md`).
    type BatchInsertSupport = sql_dialect::batch_insert_support::DoesNotSupportBatchInsert;
    type ConcatClause = sql_dialect::concat_clause::ConcatWithPipesClause;
    type DefaultValueClauseForInsert = sql_dialect::default_value_clause::AnsiDefaultValueClause;
    type EmptyFromClauseSyntax = sql_dialect::from_clause_syntax::AnsiSqlFromClauseSyntax;
    type ExistsSyntax = sql_dialect::exists_syntax::AnsiSqlExistsSyntax;
    type ArrayComparison = sql_dialect::array_comparison::AnsiSqlArrayComparison;
    type SelectStatementSyntax = sql_dialect::select_statement_syntax::AnsiSqlSelectStatement;
    type AliasSyntax = sql_dialect::alias_syntax::AsAliasSyntax;
    type WindowFrameClauseGroupSupport =
        sql_dialect::window_frame_clause_group_support::NoGroupWindowFrameUnit;
    type WindowFrameExclusionSupport =
        sql_dialect::window_frame_exclusion_support::NoFrameFrameExclusionSupport;
    type AggregateFunctionExpressions =
        sql_dialect::aggregate_function_expressions::NoAggregateFunctionExpressions;
    type BuiltInWindowFunctionRequireOrder =
        sql_dialect::built_in_window_function_require_order::NoOrderRequired;
}

impl DieselReserveSpecialization for ClickHouse {}
impl TrustedBackend for ClickHouse {}

impl<L, O> QueryFragment<ClickHouse> for LimitOffsetClause<L, O>
where
    L: QueryFragment<ClickHouse>,
    O: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        self.limit_clause.walk_ast(out.reborrow())?;
        self.offset_clause.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for BoxedLimitOffsetClause<'_, ClickHouse> {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if let Some(ref limit) = self.limit {
            limit.walk_ast(out.reborrow())?;
        }
        if let Some(ref offset) = self.offset {
            offset.walk_ast(out.reborrow())?;
        }
        Ok(())
    }
}

/// Metadata for one ClickHouse HTTP parameter shape found in rendered SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedParameterMetadata {
    /// Parameter name from `{name:Type}`.
    pub name: String,
    /// ClickHouse type text from `{name:Type}`.
    pub type_name: String,
    /// Number of times this exact `{name:Type}` pair appears in the SQL.
    pub occurrences: usize,
}

/// Metadata extracted from rendered ClickHouse SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSqlMetadata {
    /// Number of positional `?` placeholders outside quoted strings/comments.
    pub positional_bind_count: usize,
    /// ClickHouse type for each Diesel-collected positional bind, in bind order.
    pub positional_bind_types: Vec<String>,
    /// ClickHouse HTTP parameter names such as `tenant_id` from
    /// `{tenant_id:String}`, in first-seen order.
    pub named_parameters: Vec<String>,
    /// Named HTTP parameter details by first-seen `{name:Type}` pair.
    pub named_parameter_details: Vec<NamedParameterMetadata>,
}

/// Rendered SQL plus lightweight bind metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSql {
    /// SQL produced by the Diesel AST.
    pub sql: String,
    /// Placeholder/parameter metadata parsed from [`Self::sql`].
    pub metadata: RenderedSqlMetadata,
}

impl RenderedSql {
    /// Number of positional `?` placeholders outside quoted strings/comments.
    pub fn positional_bind_count(&self) -> usize {
        self.metadata.positional_bind_count
    }

    /// ClickHouse type for each Diesel-collected positional bind, in bind order.
    pub fn positional_bind_types(&self) -> &[String] {
        &self.metadata.positional_bind_types
    }

    /// ClickHouse HTTP parameter names in first-seen order.
    pub fn named_parameters(&self) -> &[String] {
        &self.metadata.named_parameters
    }

    /// ClickHouse HTTP parameter details by first-seen `{name:Type}` pair.
    pub fn named_parameter_details(&self) -> &[NamedParameterMetadata] {
        &self.metadata.named_parameter_details
    }
}

/// Render any Diesel AST node as ClickHouse SQL without the debug bind comment.
///
/// Bind parameters are represented as `?`, matching the placeholder style used
/// by many ClickHouse clients. [`AsyncClickHouseConnection`](crate::AsyncClickHouseConnection)
/// collects the corresponding Diesel bind values and sends the resulting query
/// over ClickHouse HTTP.
pub fn to_sql<T>(query: &T) -> QueryResult<String>
where
    T: QueryFragment<ClickHouse>,
{
    let backend = ClickHouse;
    let mut query_builder = ClickHouseQueryBuilder::default();
    query.to_sql(&mut query_builder, &backend)?;
    Ok(query_builder.finish())
}

/// Render SQL and return lightweight placeholder metadata.
///
/// This helper is intentionally transport-agnostic: it does not own the bind
/// values, but it lets callers using [`to_sql`] with `clickhouse::Client` assert
/// that their later `.bind(...)` and `.param(...)` calls match the rendered SQL.
/// It reports positional bind types collected from Diesel plus ClickHouse
/// `{name:Type}` HTTP parameter names, types, and occurrence counts scanned
/// from the rendered SQL text.
pub fn to_sql_with_metadata<T>(query: &T) -> QueryResult<RenderedSql>
where
    T: QueryFragment<ClickHouse>,
{
    let sql = to_sql(query)?;
    let mut metadata = analyze_rendered_sql(&sql);

    let backend = ClickHouse;
    let mut metadata_lookup = ();
    let mut bind_collector = RawBytesBindCollector::<ClickHouse>::new();
    query.collect_binds(&mut bind_collector, &mut metadata_lookup, &backend)?;
    metadata.positional_bind_types = bind_collector
        .metadata
        .iter()
        .filter(|metadata| !metadata.is_named_parameter())
        .map(|metadata| metadata.parameter_type().to_owned())
        .collect();

    Ok(RenderedSql { sql, metadata })
}

/// Extract positional and ClickHouse named parameter metadata from SQL.
///
/// This scanner sees only SQL text, so [`RenderedSqlMetadata::positional_bind_types`]
/// is empty in its return value. Use [`to_sql_with_metadata`] when Diesel bind
/// type metadata is needed. The scanner ignores placeholders inside
/// single-quoted strings, double-quoted identifiers/strings, backtick
/// identifiers, line comments, and block comments.
pub fn analyze_rendered_sql(sql: &str) -> RenderedSqlMetadata {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        Backtick,
        LineComment,
        BlockComment,
    }

    let mut state = State::Normal;
    let mut positional_bind_count = 0;
    let mut named_parameters = Vec::new();
    let mut named_parameter_details: Vec<NamedParameterMetadata> = Vec::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut idx = 0;

    while idx < chars.len() {
        let ch = chars[idx];
        let next = chars.get(idx + 1).copied();
        match state {
            State::Normal => match (ch, next) {
                ('?', _) => positional_bind_count += 1,
                ('\'', _) => state = State::SingleQuote,
                ('"', _) => state = State::DoubleQuote,
                ('`', _) => state = State::Backtick,
                ('-', Some('-')) => {
                    state = State::LineComment;
                    idx += 1;
                }
                ('/', Some('*')) => {
                    state = State::BlockComment;
                    idx += 1;
                }
                ('{', _) => {
                    if let Some((name, type_name, end_idx)) = parse_named_parameter(&chars, idx) {
                        if !named_parameters.iter().any(|seen| seen == &name) {
                            named_parameters.push(name.clone());
                        }
                        if let Some(existing) =
                            named_parameter_details.iter_mut().find(|parameter| {
                                parameter.name == name && parameter.type_name == type_name
                            })
                        {
                            existing.occurrences += 1;
                        } else {
                            named_parameter_details.push(NamedParameterMetadata {
                                name,
                                type_name,
                                occurrences: 1,
                            });
                        }
                        idx = end_idx;
                    }
                }
                _ => {}
            },
            State::SingleQuote => match (ch, next) {
                ('\\', Some(_)) => idx += 1,
                ('\'', Some('\'')) => idx += 1,
                ('\'', _) => state = State::Normal,
                _ => {}
            },
            State::DoubleQuote => match (ch, next) {
                ('\\', Some(_)) => idx += 1,
                ('"', Some('"')) => idx += 1,
                ('"', _) => state = State::Normal,
                _ => {}
            },
            State::Backtick => match (ch, next) {
                ('`', Some('`')) => idx += 1,
                ('`', _) => state = State::Normal,
                _ => {}
            },
            State::LineComment => {
                if ch == '\n' {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                if ch == '*' && next == Some('/') {
                    state = State::Normal;
                    idx += 1;
                }
            }
        }
        idx += 1;
    }

    RenderedSqlMetadata {
        positional_bind_count,
        positional_bind_types: Vec::new(),
        named_parameters,
        named_parameter_details,
    }
}

fn parse_named_parameter(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let mut idx = start + 1;
    let first = *chars.get(idx)?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }

    let mut name = String::new();
    name.push(first);
    idx += 1;
    while let Some(ch) = chars.get(idx).copied() {
        if ch == ':' {
            break;
        }
        if !(ch == '_' || ch.is_ascii_alphanumeric()) {
            return None;
        }
        name.push(ch);
        idx += 1;
    }

    if chars.get(idx).copied() != Some(':') {
        return None;
    }

    idx += 1;
    let type_start = idx;
    while let Some(ch) = chars.get(idx).copied() {
        if ch == '}' {
            if idx == type_start {
                return None;
            }
            let type_name = chars[type_start..idx]
                .iter()
                .collect::<String>()
                .trim()
                .to_owned();
            if type_name.is_empty() {
                return None;
            }
            return Some((name, type_name, idx));
        }
        if ch == '\n' || ch == '\r' || ch == '\'' || ch == '"' || ch == '`' || ch == '{' {
            return None;
        }
        idx += 1;
    }

    None
}
