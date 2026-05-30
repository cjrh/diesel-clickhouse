//! ClickHouse-specific Diesel SQL type markers.

use std::marker::PhantomData;

use diesel::query_builder::QueryId;
use diesel::sql_types::is_nullable;
use diesel::sql_types::{HasSqlType, SingleValue, SqlType};

use crate::backend::{ClickHouse, ClickHouseTypeMetadata};

/// ClickHouse `UInt8`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct UInt8;

/// ClickHouse `UInt16`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct UInt16;

/// ClickHouse `UInt32`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct UInt32;

/// ClickHouse `UInt64`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct UInt64;

/// ClickHouse `Int8`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Int8;

/// ClickHouse `DateTime64`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct DateTime64;

/// ClickHouse `UUID`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Uuid;

/// ClickHouse `JSON` / `Object('json')`-style values.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Json;

/// ClickHouse `Nothing`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Nothing;

/// ClickHouse `Array(T)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Array<ST>(PhantomData<ST>);

/// ClickHouse `LowCardinality(T)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LowCardinality<ST>(PhantomData<ST>);

/// ClickHouse `Map(K, V)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Map<K, V>(PhantomData<(K, V)>);

impl<ST> SqlType for Array<ST>
where
    ST: SqlType,
{
    type IsNull = is_nullable::NotNull;
}

impl<ST> SingleValue for Array<ST> where ST: SqlType {}

impl<ST> QueryId for Array<ST>
where
    ST: QueryId,
{
    type QueryId = Array<ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = ST::HAS_STATIC_QUERY_ID;
}

impl<ST> SqlType for LowCardinality<ST>
where
    ST: SqlType,
{
    type IsNull = is_nullable::NotNull;
}

impl<ST> SingleValue for LowCardinality<ST> where ST: SqlType {}

impl<ST> QueryId for LowCardinality<ST>
where
    ST: QueryId,
{
    type QueryId = LowCardinality<ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = ST::HAS_STATIC_QUERY_ID;
}

impl<K, V> SqlType for Map<K, V>
where
    K: SqlType,
    V: SqlType,
{
    type IsNull = is_nullable::NotNull;
}

impl<K, V> SingleValue for Map<K, V>
where
    K: SqlType,
    V: SqlType,
{
}

impl<K, V> QueryId for Map<K, V>
where
    K: QueryId,
    V: QueryId,
{
    type QueryId = Map<K::QueryId, V::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = K::HAS_STATIC_QUERY_ID && V::HAS_STATIC_QUERY_ID;
}

macro_rules! clickhouse_type {
    ($sql_type:ty => $name:literal) => {
        impl HasSqlType<$sql_type> for ClickHouse {
            fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
                ClickHouseTypeMetadata::new($name)
            }
        }
    };
}

clickhouse_type!(UInt8 => "UInt8");
clickhouse_type!(UInt16 => "UInt16");
clickhouse_type!(UInt32 => "UInt32");
clickhouse_type!(UInt64 => "UInt64");
clickhouse_type!(Int8 => "Int8");
clickhouse_type!(DateTime64 => "DateTime64");
clickhouse_type!(Uuid => "UUID");
clickhouse_type!(Json => "JSON");
clickhouse_type!(Nothing => "Nothing");

impl<ST> HasSqlType<Array<ST>> for ClickHouse
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST>,
{
    fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        ClickHouseTypeMetadata::new("Array")
    }
}

impl<ST> HasSqlType<LowCardinality<ST>> for ClickHouse
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST>,
{
    fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        ClickHouseTypeMetadata::new("LowCardinality")
    }
}

impl<K, V> HasSqlType<Map<K, V>> for ClickHouse
where
    K: SqlType,
    V: SqlType,
    ClickHouse: HasSqlType<K> + HasSqlType<V>,
{
    fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        ClickHouseTypeMetadata::new("Map")
    }
}
