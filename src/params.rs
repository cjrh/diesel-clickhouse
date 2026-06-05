//! Named ClickHouse HTTP parameters that still bind through Diesel.
//!
//! `named_param::<ST, _>("q", value)` renders a ClickHouse `{q:Type}` server
//! parameter and stores the value in Diesel's bind collector. Reusing/cloning
//! the expression renders the same `{q:Type}` reference each time; the async
//! connection de-duplicates identical hidden binds into one HTTP setting.

use std::fmt;
use std::io::Write;
use std::marker::PhantomData;

use diesel::expression::{
    AppearsOnTable, Expression, SelectableExpression, ValidGrouping, is_aggregate,
};
use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::{HasSqlType, SingleValue, SqlType};

use crate::backend::ClickHouse;

pub(crate) const NAMED_PARAMETER_MAGIC: &[u8] = b"DCH_NAMED_PARAM\0";
pub(crate) const INTERNAL_PARAMETER_PREFIX: &str = "__diesel_clickhouse_";

/// SQL type marker used internally for hidden named-parameter bind collection.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NamedParameterSqlType<ST>(PhantomData<ST>);

/// A reusable ClickHouse `{name:Type}` HTTP parameter expression.
///
/// Construct one with [`named_param`] or [`ch_param`]. The expression has SQL
/// type `ST`, so it can be passed anywhere an expression of that type is
/// accepted. When executed through [`AsyncClickHouseConnection`], the value is
/// bound once as a ClickHouse HTTP setting (`param_name=value`) even if the
/// expression is cloned and referenced multiple times in the query.
///
/// [`AsyncClickHouseConnection`]: crate::AsyncClickHouseConnection
#[derive(Clone)]
pub struct NamedParam<ST, T> {
    name: String,
    value: T,
    _sql_type: PhantomData<ST>,
}

/// Bind `value` as a reusable ClickHouse HTTP parameter named `name`.
///
/// The parameter name is validated as a bare ClickHouse identifier and renders
/// as `{name:Type}`, where `Type` is derived from the Diesel SQL type `ST`.
/// Values are still collected by Diesel when the query executes through
/// [`AsyncClickHouseConnection`](crate::AsyncClickHouseConnection).
pub fn named_param<ST, T>(name: impl Into<String>, value: T) -> NamedParam<ST, T> {
    NamedParam {
        name: name.into(),
        value,
        _sql_type: PhantomData,
    }
}

/// Short alias for [`named_param`].
pub fn ch_param<ST, T>(name: impl Into<String>, value: T) -> NamedParam<ST, T> {
    named_param(name, value)
}

impl<ST, T> NamedParam<ST, T> {
    /// The ClickHouse HTTP parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl<ST, T> fmt::Debug for NamedParam<ST, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedParam")
            .field("name", &self.name)
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl<ST, T> Expression for NamedParam<ST, T>
where
    ST: SqlType + SingleValue,
{
    type SqlType = ST;
}

impl<ST, T> QueryFragment<ClickHouse> for NamedParam<ST, T>
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST> + HasSqlType<NamedParameterSqlType<ST>>,
    Self: ToSql<NamedParameterSqlType<ST>, ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        validate_parameter_name(&self.name)?;

        let mut lookup = ();
        let metadata = <ClickHouse as HasSqlType<ST>>::metadata(&mut lookup);
        out.push_sql("{");
        out.push_sql(&self.name);
        out.push_sql(":");
        out.push_sql(metadata.parameter_type());
        out.push_sql("}");
        out.push_bind_param_value_only::<NamedParameterSqlType<ST>, _>(self)
    }
}

impl<ST, T> QueryId for NamedParam<ST, T>
where
    ST: QueryId,
{
    type QueryId = NamedParam<ST::QueryId, ()>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<ST, T, QS> AppearsOnTable<QS> for NamedParam<ST, T> where NamedParam<ST, T>: Expression {}
impl<ST, T, QS> SelectableExpression<QS> for NamedParam<ST, T> where
    NamedParam<ST, T>: AppearsOnTable<QS>
{
}

impl<ST, T, GB> ValidGrouping<GB> for NamedParam<ST, T> {
    type IsAggregate = is_aggregate::Never;
}

impl<ST> SqlType for NamedParameterSqlType<ST>
where
    ST: SqlType,
{
    type IsNull = diesel::sql_types::is_nullable::NotNull;
}

impl<ST> SingleValue for NamedParameterSqlType<ST> where ST: SqlType {}

impl<ST> QueryId for NamedParameterSqlType<ST>
where
    ST: QueryId,
{
    type QueryId = NamedParameterSqlType<ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<ST> HasSqlType<NamedParameterSqlType<ST>> for ClickHouse
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST>,
{
    fn metadata(lookup: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        <ClickHouse as HasSqlType<ST>>::metadata(lookup).into_named_parameter()
    }
}

impl<ST, T> ToSql<NamedParameterSqlType<ST>, ClickHouse> for NamedParam<ST, T>
where
    ST: SqlType,
    T: ToSql<ST, ClickHouse>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> diesel::serialize::Result {
        out.write_all(NAMED_PARAMETER_MAGIC)?;
        let name = self.name.as_bytes();
        let name_len: u32 = name
            .len()
            .try_into()
            .map_err(|_| "ClickHouse named parameter name is too long")?;
        out.write_all(&name_len.to_be_bytes())?;
        out.write_all(name)?;

        match self.value.to_sql(out)? {
            IsNull::No => Ok(IsNull::No),
            IsNull::Yes => Err("ClickHouse named parameters cannot be NULL".into()),
        }
    }
}

fn validate_parameter_name(value: &str) -> QueryResult<()> {
    if value.starts_with(INTERNAL_PARAMETER_PREFIX) {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse parameter name uses reserved diesel-clickhouse prefix: {value:?}")
                .into(),
        ));
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(Error::QueryBuilderError(
            "empty ClickHouse parameter name".into(),
        ));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse parameter name: {value:?}").into(),
        ));
    }
    if chars.any(|ch| !(ch == '_' || ch.is_ascii_alphanumeric())) {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse parameter name: {value:?}").into(),
        ));
    }
    Ok(())
}

pub(crate) fn split_named_parameter_bind(bytes: &[u8]) -> QueryResult<(&str, &[u8])> {
    if !bytes.starts_with(NAMED_PARAMETER_MAGIC) {
        return Err(Error::SerializationError(
            "invalid ClickHouse named parameter bind payload".into(),
        ));
    }
    let rest = &bytes[NAMED_PARAMETER_MAGIC.len()..];
    let Some(len_bytes) = rest.get(..4) else {
        return Err(Error::SerializationError(
            "truncated ClickHouse named parameter bind payload".into(),
        ));
    };
    let name_len = u32::from_be_bytes(len_bytes.try_into().expect("slice length checked")) as usize;
    let rest = &rest[4..];
    let Some(name_bytes) = rest.get(..name_len) else {
        return Err(Error::SerializationError(
            "truncated ClickHouse named parameter name".into(),
        ));
    };
    let name = std::str::from_utf8(name_bytes).map_err(|err| {
        Error::SerializationError(Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
    })?;
    Ok((name, &rest[name_len..]))
}
