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

/// ClickHouse vector literal rendered as `[x, y, ...]`.
#[derive(Debug, Clone)]
pub struct VectorLiteral<ST> {
    values: Vec<f64>,
    _sql_type: PhantomData<ST>,
}

impl<ST> VectorLiteral<ST> {
    fn new(values: Vec<f64>) -> Self {
        Self {
            values,
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

impl<ST, GB> ValidGrouping<GB> for VectorLiteral<ST> {
    type IsAggregate = diesel::expression::is_aggregate::No;
}

impl<ST, QS> AppearsOnTable<QS> for VectorLiteral<ST> where Self: Expression {}
impl<ST, QS> SelectableExpression<QS> for VectorLiteral<ST> where Self: AppearsOnTable<QS> {}

impl<ST> QueryId for VectorLiteral<ST> {
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
