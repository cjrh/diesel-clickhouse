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

/// ClickHouse `UInt128`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct UInt128;

/// ClickHouse `UInt256`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct UInt256;

/// ClickHouse `Int8`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Int8;

/// ClickHouse `Int128`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Int128;

/// ClickHouse `Int256`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Int256;

/// ClickHouse `DateTime64`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct DateTime64;

/// ClickHouse `BFloat16`, commonly used for compact vector embeddings.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct BFloat16;

/// ClickHouse `Decimal32(scale)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decimal32<const SCALE: u8>;

/// ClickHouse `Decimal64(scale)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decimal64<const SCALE: u8>;

/// ClickHouse `Decimal128(scale)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decimal128<const SCALE: u8>;

/// ClickHouse `Decimal256(scale)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decimal256<const SCALE: u8>;

/// ClickHouse `Enum8(...)`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Enum8;

/// ClickHouse `Enum16(...)`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Enum16;

/// ClickHouse `UUID`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Uuid;

/// ClickHouse `JSON` / `Object('json')`-style values.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct Json;

/// ClickHouse `IPv4`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct IPv4;

/// ClickHouse `IPv6`.
#[derive(Debug, Clone, Copy, Default, QueryId, SqlType)]
pub struct IPv6;

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

/// ClickHouse `Tuple(...)` stored in a single column.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tuple<ST>(PhantomData<ST>);

/// ClickHouse `Nested(...)` stored in a single column family.
#[derive(Debug, Clone, Copy, Default)]
pub struct Nested<ST>(PhantomData<ST>);

/// ClickHouse `AggregateFunction(...)` state whose finalized value has type `ST`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AggregateFunction<ST>(PhantomData<ST>);

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

macro_rules! decimal_type {
    ($name:ident) => {
        impl<const SCALE: u8> SqlType for $name<SCALE> {
            type IsNull = is_nullable::NotNull;
        }

        impl<const SCALE: u8> SingleValue for $name<SCALE> {}

        impl<const SCALE: u8> QueryId for $name<SCALE> {
            type QueryId = $name<SCALE>;
            const HAS_STATIC_QUERY_ID: bool = true;
        }
    };
}

decimal_type!(Decimal32);
decimal_type!(Decimal64);
decimal_type!(Decimal128);
decimal_type!(Decimal256);

impl<ST> SqlType for Tuple<ST>
where
    ST: SqlType,
{
    type IsNull = is_nullable::NotNull;
}

impl<ST> SingleValue for Tuple<ST> where ST: SqlType {}

impl<ST> QueryId for Tuple<ST>
where
    ST: QueryId,
{
    type QueryId = Tuple<ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = ST::HAS_STATIC_QUERY_ID;
}

impl<ST> SqlType for Nested<ST>
where
    ST: SqlType,
{
    type IsNull = is_nullable::NotNull;
}

impl<ST> SingleValue for Nested<ST> where ST: SqlType {}

impl<ST> QueryId for Nested<ST>
where
    ST: QueryId,
{
    type QueryId = Nested<ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = ST::HAS_STATIC_QUERY_ID;
}

impl<ST> SqlType for AggregateFunction<ST>
where
    ST: SqlType,
{
    type IsNull = is_nullable::NotNull;
}

impl<ST> SingleValue for AggregateFunction<ST> where ST: SqlType {}

impl<ST> QueryId for AggregateFunction<ST>
where
    ST: QueryId,
{
    type QueryId = AggregateFunction<ST::QueryId>;
    const HAS_STATIC_QUERY_ID: bool = ST::HAS_STATIC_QUERY_ID;
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
clickhouse_type!(UInt128 => "UInt128");
clickhouse_type!(UInt256 => "UInt256");
clickhouse_type!(Int8 => "Int8");
clickhouse_type!(Int128 => "Int128");
clickhouse_type!(Int256 => "Int256");
clickhouse_type!(DateTime64 => "DateTime64");
clickhouse_type!(BFloat16 => "BFloat16");
clickhouse_type!(Enum8 => "Enum8");
clickhouse_type!(Enum16 => "Enum16");
clickhouse_type!(Uuid => "UUID");
clickhouse_type!(Json => "JSON");
clickhouse_type!(IPv4 => "IPv4");
clickhouse_type!(IPv6 => "IPv6");
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

macro_rules! has_decimal_type {
    ($name:ident => $metadata:literal) => {
        impl<const SCALE: u8> HasSqlType<$name<SCALE>> for ClickHouse {
            fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
                ClickHouseTypeMetadata::new($metadata)
            }
        }
    };
}

has_decimal_type!(Decimal32 => "Decimal32");
has_decimal_type!(Decimal64 => "Decimal64");
has_decimal_type!(Decimal128 => "Decimal128");
has_decimal_type!(Decimal256 => "Decimal256");

impl<ST> HasSqlType<Tuple<ST>> for ClickHouse
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST>,
{
    fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        ClickHouseTypeMetadata::new("Tuple")
    }
}

impl<ST> HasSqlType<Nested<ST>> for ClickHouse
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST>,
{
    fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        ClickHouseTypeMetadata::new("Nested")
    }
}

impl<ST> HasSqlType<AggregateFunction<ST>> for ClickHouse
where
    ST: SqlType,
    ClickHouse: HasSqlType<ST>,
{
    fn metadata(_: &mut Self::MetadataLookup) -> Self::TypeMetadata {
        ClickHouseTypeMetadata::new("AggregateFunction")
    }
}
