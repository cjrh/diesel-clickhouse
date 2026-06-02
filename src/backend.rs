//! Lightweight ClickHouse backend for Diesel SQL rendering.
//!
//! The backend gives Diesel a ClickHouse-shaped SQL dialect (`backtick`
//! identifiers, `?` placeholders and ANSI-ish `SELECT` layout).  It is shared
//! by the render-only [`to_sql`] helper and the HTTP-backed
//! [`ClickHouseConnection`](crate::ClickHouseConnection).

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
}

impl ClickHouseTypeMetadata {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            parameter_type: Cow::Borrowed(name),
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
        }
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
    // through Diesel, and high-throughput multi-row ingestion uses the
    // `clickhouse` client's RowBinary inserter (see `docs/USAGE.md`).
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

/// Render any Diesel AST node as ClickHouse SQL without the debug bind comment.
///
/// Bind parameters are represented as `?`, matching the placeholder style used
/// by many ClickHouse clients. [`ClickHouseConnection`](crate::ClickHouseConnection)
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
