//! Helpers for ClickHouse vector-search expressions.

use std::marker::PhantomData;

use diesel::backend::Backend;
use diesel::expression::{AppearsOnTable, Expression, SelectableExpression, ValidGrouping};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::sql_types::{Double, Float, SqlType};

use crate::types::Array;

/// Build a ClickHouse array literal typed as `Array(Float32)`.
pub fn vector_f32<I>(values: I) -> VectorLiteral<Float>
where
    I: IntoIterator<Item = f32>,
{
    VectorLiteral::new(values.into_iter().map(f64::from).collect())
}

/// Build a ClickHouse array literal typed as `Array(Float64)`.
pub fn vector_f64<I>(values: I) -> VectorLiteral<Double>
where
    I: IntoIterator<Item = f64>,
{
    VectorLiteral::new(values.into_iter().collect())
}

/// Reinterpret a binary string expression as `Array(Float32)`.
///
/// ClickHouse expects bytes in little-endian element order. For clients that
/// cannot bind arbitrary bytes as a string literal, use [`vector_f32_hex`]
/// with [`vector_f32_le_hex`] instead.
pub fn vector_f32_binary<Expr>(bytes: Expr) -> VectorBytes<Expr, Float>
where
    Expr: Expression,
{
    VectorBytes::new(bytes, "Float32", VectorBytesEncoding::Raw)
}

/// Reinterpret a binary string expression as `Array(Float64)`.
pub fn vector_f64_binary<Expr>(bytes: Expr) -> VectorBytes<Expr, Double>
where
    Expr: Expression,
{
    VectorBytes::new(bytes, "Float64", VectorBytesEncoding::Raw)
}

/// Decode a hex string expression with `unhex` and reinterpret it as `Array(Float32)`.
pub fn vector_f32_hex<Expr>(hex: Expr) -> VectorBytes<Expr, Float>
where
    Expr: Expression,
{
    VectorBytes::new(hex, "Float32", VectorBytesEncoding::Hex)
}

/// Decode a hex string expression with `unhex` and reinterpret it as `Array(Float64)`.
pub fn vector_f64_hex<Expr>(hex: Expr) -> VectorBytes<Expr, Double>
where
    Expr: Expression,
{
    VectorBytes::new(hex, "Float64", VectorBytesEncoding::Hex)
}

/// Convert `f32` vector values into ClickHouse-compatible little-endian bytes.
pub fn vector_f32_le_bytes<I>(values: I) -> Vec<u8>
where
    I: IntoIterator<Item = f32>,
{
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// Convert `f64` vector values into ClickHouse-compatible little-endian bytes.
pub fn vector_f64_le_bytes<I>(values: I) -> Vec<u8>
where
    I: IntoIterator<Item = f64>,
{
    values.into_iter().flat_map(f64::to_le_bytes).collect()
}

/// Convert `f32` vector values into a lower-case hex string of little-endian bytes.
pub fn vector_f32_le_hex<I>(values: I) -> String
where
    I: IntoIterator<Item = f32>,
{
    bytes_to_hex(vector_f32_le_bytes(values))
}

/// Convert `f64` vector values into a lower-case hex string of little-endian bytes.
pub fn vector_f64_le_hex<I>(values: I) -> String
where
    I: IntoIterator<Item = f64>,
{
    bytes_to_hex(vector_f64_le_bytes(values))
}

/// ClickHouse vector literal rendered as `[x, y, ...]`.
#[derive(Debug, Clone)]
pub struct VectorLiteral<ST> {
    values: Vec<f64>,
    _sql_type: PhantomData<ST>,
}

/// ClickHouse binary-vector reinterpret expression.
#[derive(Debug, Clone)]
pub struct VectorBytes<Expr, ST> {
    expr: Expr,
    element_type: &'static str,
    encoding: VectorBytesEncoding,
    _sql_type: PhantomData<ST>,
}

/// How the input expression should be decoded before reinterpretation.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum VectorBytesEncoding {
    /// The expression already evaluates to a binary `String`/`FixedString` value.
    Raw,
    /// The expression evaluates to a hex string and should be wrapped in `unhex(...)`.
    Hex,
}

impl<ST> VectorLiteral<ST> {
    fn new(values: Vec<f64>) -> Self {
        Self {
            values,
            _sql_type: PhantomData,
        }
    }
}

impl<Expr, ST> VectorBytes<Expr, ST> {
    fn new(expr: Expr, element_type: &'static str, encoding: VectorBytesEncoding) -> Self {
        Self {
            expr,
            element_type,
            encoding,
            _sql_type: PhantomData,
        }
    }
}

impl<ST> Expression for VectorLiteral<ST>
where
    ST: SqlType,
{
    type SqlType = Array<ST>;
}

impl<Expr, ST> Expression for VectorBytes<Expr, ST>
where
    Expr: Expression,
    ST: SqlType,
{
    type SqlType = Array<ST>;
}

impl<ST, GB> ValidGrouping<GB> for VectorLiteral<ST> {
    type IsAggregate = diesel::expression::is_aggregate::No;
}

impl<Expr, ST, GB> ValidGrouping<GB> for VectorBytes<Expr, ST>
where
    Expr: ValidGrouping<GB>,
{
    type IsAggregate = Expr::IsAggregate;
}

impl<ST, QS> AppearsOnTable<QS> for VectorLiteral<ST> where Self: Expression {}
impl<Expr, ST, QS> AppearsOnTable<QS> for VectorBytes<Expr, ST>
where
    Expr: AppearsOnTable<QS>,
    Self: Expression,
{
}
impl<ST, QS> SelectableExpression<QS> for VectorLiteral<ST> where Self: AppearsOnTable<QS> {}
impl<Expr, ST, QS> SelectableExpression<QS> for VectorBytes<Expr, ST> where Self: AppearsOnTable<QS> {}

impl<ST> QueryId for VectorLiteral<ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Expr, ST> QueryId for VectorBytes<Expr, ST> {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<ST, DB> QueryFragment<DB> for VectorLiteral<ST>
where
    DB: Backend,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        if self.values.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse vector literal requires at least one value".into(),
            ));
        }

        out.push_sql("[");
        for (idx, value) in self.values.iter().enumerate() {
            if !value.is_finite() {
                return Err(Error::QueryBuilderError(
                    format!("ClickHouse vector literal value must be finite, got {value}").into(),
                ));
            }
            if idx > 0 {
                out.push_sql(", ");
            }
            out.push_sql(&value.to_string());
        }
        out.push_sql("]");
        Ok(())
    }
}

impl<Expr, ST, DB> QueryFragment<DB> for VectorBytes<Expr, ST>
where
    DB: Backend,
    Expr: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql("reinterpret(");
        if self.encoding == VectorBytesEncoding::Hex {
            out.push_sql("unhex(");
        }
        self.expr.walk_ast(out.reborrow())?;
        if self.encoding == VectorBytesEncoding::Hex {
            out.push_sql(")");
        }
        out.push_sql(", '");
        out.push_sql("Array(");
        out.push_sql(self.element_type);
        out.push_sql(")");
        out.push_sql("')");
        Ok(())
    }
}

fn bytes_to_hex(bytes: Vec<u8>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
