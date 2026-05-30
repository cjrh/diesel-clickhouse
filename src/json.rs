//! ClickHouse JSON path helpers for variable-length `indices_or_keys` calls.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{AppearsOnTable, Expression, SelectableExpression, ValidGrouping};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{BigInt, Bool, Double, SingleValue, SqlType, Text};

use crate::types::UInt64;

/// One ClickHouse JSON path argument: either an object key or an array index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathSegment {
    Key(String),
    Index(i64),
}

impl JsonPathSegment {
    /// Build a key segment.
    pub fn key(value: impl Into<String>) -> Self {
        Self::Key(value.into())
    }

    /// Build an array-index segment.
    pub fn index(value: i64) -> Self {
        Self::Index(value)
    }
}

impl From<&str> for JsonPathSegment {
    fn from(value: &str) -> Self {
        Self::key(value)
    }
}

impl From<String> for JsonPathSegment {
    fn from(value: String) -> Self {
        Self::key(value)
    }
}

impl From<i32> for JsonPathSegment {
    fn from(value: i32) -> Self {
        Self::index(value.into())
    }
}

impl From<i64> for JsonPathSegment {
    fn from(value: i64) -> Self {
        Self::index(value)
    }
}

/// Render a JSON function with `json[, indices_or_keys]...` shape.
#[derive(Debug, Clone)]
pub struct JsonPathFunction<Json, ST> {
    function: &'static str,
    json: Json,
    path: Vec<JsonPathSegment>,
    _sql_type: PhantomData<ST>,
}

/// Render `JSONExtractString(json, path...)`.
pub fn json_extract_string_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, Text>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractString", json, path)
}

/// Render `JSONExtractInt(json, path...)`.
pub fn json_extract_int_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, BigInt>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractInt", json, path)
}

/// Render `JSONExtractUInt(json, path...)`.
pub fn json_extract_uint_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, UInt64>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractUInt", json, path)
}

/// Render `JSONExtractFloat(json, path...)`.
pub fn json_extract_float_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, Double>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractFloat", json, path)
}

/// Render `JSONExtractBool(json, path...)`.
pub fn json_extract_bool_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, Bool>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractBool", json, path)
}

/// Render `JSONExtractRaw(json, path...)`.
pub fn json_extract_raw_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, Text>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractRaw", json, path)
}

/// Render `JSONExtractStringCaseInsensitive(json, path...)`.
pub fn json_extract_string_ci_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, Text>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractStringCaseInsensitive", json, path)
}

/// Render `JSONExtractIntCaseInsensitive(json, path...)`.
pub fn json_extract_int_ci_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, BigInt>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractIntCaseInsensitive", json, path)
}

/// Render `JSONExtractRawCaseInsensitive(json, path...)`.
pub fn json_extract_raw_ci_path<Json, I, S>(json: Json, path: I) -> JsonPathFunction<Json, Text>
where
    Json: Expression,
    I: IntoIterator<Item = S>,
    S: Into<JsonPathSegment>,
{
    JsonPathFunction::new("JSONExtractRawCaseInsensitive", json, path)
}

impl<Json, ST> JsonPathFunction<Json, ST> {
    fn new<I, S>(function: &'static str, json: Json, path: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<JsonPathSegment>,
    {
        Self {
            function,
            json,
            path: path.into_iter().map(Into::into).collect(),
            _sql_type: PhantomData,
        }
    }
}

impl<Json, ST> Expression for JsonPathFunction<Json, ST>
where
    Json: Expression,
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<Json, ST, GB> ValidGrouping<GB> for JsonPathFunction<Json, ST>
where
    Json: ValidGrouping<GB>,
{
    type IsAggregate = Json::IsAggregate;
}

impl<Json, ST, QS> AppearsOnTable<QS> for JsonPathFunction<Json, ST>
where
    Json: AppearsOnTable<QS>,
    Self: Expression,
{
}

impl<Json, ST, QS> SelectableExpression<QS> for JsonPathFunction<Json, ST> where
    Self: AppearsOnTable<QS>
{
}

impl<Json, ST> QueryId for JsonPathFunction<Json, ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Json, ST, DB> QueryFragment<DB> for JsonPathFunction<Json, ST>
where
    DB: Backend,
    Json: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if self.path.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse JSON path helper requires at least one path segment".into(),
            ));
        }

        out.push_sql(self.function);
        out.push_sql("(");
        self.json.walk_ast(out.reborrow())?;
        for segment in &self.path {
            out.push_sql(", ");
            match segment {
                JsonPathSegment::Key(key) => {
                    if key.is_empty() {
                        return Err(Error::QueryBuilderError(
                            "ClickHouse JSON key path segment must not be empty".into(),
                        ));
                    }
                    push_string_literal(&mut out, key);
                }
                JsonPathSegment::Index(index) => out.push_sql(&index.to_string()),
            }
        }
        out.push_sql(")");
        Ok(())
    }
}

fn push_string_literal<DB>(out: &mut AstPass<'_, '_, DB>, value: &str)
where
    DB: Backend,
{
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
