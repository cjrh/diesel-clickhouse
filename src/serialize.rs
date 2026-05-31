//! Serialization support for the lightweight ClickHouse backend.
//!
//! These implementations are deliberately simple: values are written in a
//! textual representation suitable for clients that later bind or inline them.
//! A future `Connection` adapter can replace the bind collector if it needs a
//! native binary format.

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write;
use std::str;

use diesel::Queryable;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::{
    BigInt, Binary, Bool, Date, Double, Float, Integer, Numeric, SmallInt, SqlType, Text, Time,
    Timestamp,
};

use crate::backend::ClickHouse;
use crate::types::{
    Array, DateTime64, Decimal32, Decimal64, Decimal128, Decimal256, Dynamic, IPv4, IPv6, Int8,
    Int128, Json, Map, Tuple, UInt8, UInt16, UInt32, UInt64, UInt128, Uuid, Variant,
};

macro_rules! impl_textual_to_sql {
    ($rust:ty, $sql:ty) => {
        impl ToSql<$sql, ClickHouse> for $rust {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                write!(out, "{}", self)
                    .map(|_| IsNull::No)
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
            }
        }
    };
}

impl ToSql<Bool, ClickHouse> for bool {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
        out.write_all(if *self { b"1" } else { b"0" })
            .map(|_| IsNull::No)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

impl_textual_to_sql!(i8, Int8);
impl_textual_to_sql!(i16, SmallInt);
impl_textual_to_sql!(i32, Integer);
impl_textual_to_sql!(i64, BigInt);
impl_textual_to_sql!(i128, Int128);
impl_textual_to_sql!(u8, UInt8);
impl_textual_to_sql!(u16, UInt16);
impl_textual_to_sql!(u32, UInt32);
impl_textual_to_sql!(u64, UInt64);
impl_textual_to_sql!(u128, UInt128);
impl_textual_to_sql!(f32, Float);
impl_textual_to_sql!(f64, Double);

macro_rules! impl_string_like_to_sql {
    ($sql_type:ty) => {
        impl ToSql<$sql_type, ClickHouse> for str {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                out.write_all(self.as_bytes())
                    .map(|_| IsNull::No)
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
            }
        }

        impl ToSql<$sql_type, ClickHouse> for String {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                <str as ToSql<$sql_type, ClickHouse>>::to_sql(self.as_str(), out)
            }
        }
    };
}

impl_string_like_to_sql!(Date);
impl_string_like_to_sql!(Time);
impl_string_like_to_sql!(Timestamp);
impl_string_like_to_sql!(DateTime64);
impl_string_like_to_sql!(Uuid);
impl_string_like_to_sql!(IPv4);
impl_string_like_to_sql!(IPv6);
impl_string_like_to_sql!(Json);
impl_string_like_to_sql!(Numeric);

macro_rules! impl_decimal_to_sql {
    ($decimal:ident) => {
        impl<const SCALE: u8> ToSql<$decimal<SCALE>, ClickHouse> for str {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                out.write_all(self.as_bytes())
                    .map(|_| IsNull::No)
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
            }
        }

        impl<const SCALE: u8> ToSql<$decimal<SCALE>, ClickHouse> for String {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                <str as ToSql<$decimal<SCALE>, ClickHouse>>::to_sql(self.as_str(), out)
            }
        }
    };
}

impl_decimal_to_sql!(Decimal32);
impl_decimal_to_sql!(Decimal64);
impl_decimal_to_sql!(Decimal128);
impl_decimal_to_sql!(Decimal256);

macro_rules! impl_parse_from_sql {
    ($rust:ty, $sql:ty) => {
        impl FromSql<$sql, ClickHouse> for $rust {
            fn from_sql(
                bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
            ) -> deserialize::Result<Self> {
                let s = str::from_utf8(bytes)?;
                Ok(s.parse::<$rust>()?)
            }
        }
    };
}

impl FromSql<Bool, ClickHouse> for bool {
    fn from_sql(
        bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
    ) -> deserialize::Result<Self> {
        match str::from_utf8(bytes)? {
            "1" | "true" | "TRUE" => Ok(true),
            "0" | "false" | "FALSE" => Ok(false),
            other => Err(format!("invalid ClickHouse Bool literal: {other}").into()),
        }
    }
}

impl_parse_from_sql!(i8, Int8);
impl_parse_from_sql!(i16, SmallInt);
impl_parse_from_sql!(i32, Integer);
impl_parse_from_sql!(i64, BigInt);
impl_parse_from_sql!(i128, Int128);
impl_parse_from_sql!(u8, UInt8);
impl_parse_from_sql!(u16, UInt16);
impl_parse_from_sql!(u32, UInt32);
impl_parse_from_sql!(u64, UInt64);
impl_parse_from_sql!(u128, UInt128);
impl_parse_from_sql!(f32, Float);
impl_parse_from_sql!(f64, Double);

impl FromSql<Text, ClickHouse> for String {
    fn from_sql(
        bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
    ) -> deserialize::Result<Self> {
        Ok(str::from_utf8(bytes)?.to_owned())
    }
}

impl FromSql<Binary, ClickHouse> for Vec<u8> {
    fn from_sql(
        bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
    ) -> deserialize::Result<Self> {
        Ok(bytes.to_vec())
    }
}

macro_rules! impl_string_from_sql {
    ($sql_type:ty) => {
        impl FromSql<$sql_type, ClickHouse> for String {
            fn from_sql(
                bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
            ) -> deserialize::Result<Self> {
                Ok(str::from_utf8(bytes)?.to_owned())
            }
        }
    };
}

impl_string_from_sql!(Date);
impl_string_from_sql!(Time);
impl_string_from_sql!(Timestamp);
impl_string_from_sql!(DateTime64);
impl_string_from_sql!(Uuid);
impl_string_from_sql!(IPv4);
impl_string_from_sql!(IPv6);
impl_string_from_sql!(Json);
impl_string_from_sql!(Numeric);
impl_string_from_sql!(Dynamic);
impl<ST> FromSql<Variant<ST>, ClickHouse> for String
where
    ST: SqlType,
{
    fn from_sql(
        bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
    ) -> deserialize::Result<Self> {
        Ok(str::from_utf8(bytes)?.to_owned())
    }
}

macro_rules! impl_decimal_string_from_sql {
    ($decimal:ident) => {
        impl<const SCALE: u8> FromSql<$decimal<SCALE>, ClickHouse> for String {
            fn from_sql(
                bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
            ) -> deserialize::Result<Self> {
                Ok(str::from_utf8(bytes)?.to_owned())
            }
        }
    };
}

impl_decimal_string_from_sql!(Decimal32);
impl_decimal_string_from_sql!(Decimal64);
impl_decimal_string_from_sql!(Decimal128);
impl_decimal_string_from_sql!(Decimal256);

#[cfg(feature = "bigdecimal")]
mod bigdecimal_support {
    use std::io::Write;

    use bigdecimal::BigDecimal;
    use diesel::deserialize::{self, FromSql};
    use diesel::serialize::{self, IsNull, Output, ToSql};
    use diesel::sql_types::Numeric;

    use crate::backend::ClickHouse;
    use crate::types::{Decimal32, Decimal64, Decimal128, Decimal256};

    macro_rules! impl_bigdecimal_sql {
        ($sql_type:ty) => {
            impl ToSql<$sql_type, ClickHouse> for BigDecimal {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                    write!(out, "{}", self)
                        .map(|_| IsNull::No)
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                }
            }

            impl FromSql<$sql_type, ClickHouse> for BigDecimal {
                fn from_sql(
                    bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    Ok(std::str::from_utf8(bytes)?.parse::<BigDecimal>()?)
                }
            }
        };
    }

    macro_rules! impl_bigdecimal_decimal_type {
        ($decimal:ident) => {
            impl<const SCALE: u8> ToSql<$decimal<SCALE>, ClickHouse> for BigDecimal {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, ClickHouse>) -> serialize::Result {
                    write!(out, "{}", self)
                        .map(|_| IsNull::No)
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                }
            }

            impl<const SCALE: u8> FromSql<$decimal<SCALE>, ClickHouse> for BigDecimal {
                fn from_sql(
                    bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    Ok(std::str::from_utf8(bytes)?.parse::<BigDecimal>()?)
                }
            }
        };
    }

    impl_bigdecimal_sql!(Numeric);
    impl_bigdecimal_decimal_type!(Decimal32);
    impl_bigdecimal_decimal_type!(Decimal64);
    impl_bigdecimal_decimal_type!(Decimal128);
    impl_bigdecimal_decimal_type!(Decimal256);
}

impl<ST, T> FromSql<Array<ST>, ClickHouse> for Vec<T>
where
    T: FromSql<ST, ClickHouse>,
{
    fn from_sql(
        bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
    ) -> deserialize::Result<Self> {
        let value = str::from_utf8(bytes)?;
        let items = parse_clickhouse_list(value, '[', ']')?;
        items
            .into_iter()
            .map(|item| {
                let value = decode_clickhouse_scalar(item)?;
                T::from_nullable_sql(value.as_deref())
            })
            .collect()
    }
}

impl<KST, VST, K, V> FromSql<Map<KST, VST>, ClickHouse> for BTreeMap<K, V>
where
    K: FromSql<KST, ClickHouse> + Ord,
    V: FromSql<VST, ClickHouse>,
{
    fn from_sql(
        bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
    ) -> deserialize::Result<Self> {
        let value = str::from_utf8(bytes)?;
        parse_clickhouse_map(value)?
            .into_iter()
            .map(|(key, value)| {
                let key = decode_clickhouse_scalar(key)?;
                let value = decode_clickhouse_scalar(value)?;
                let key = K::from_nullable_sql(key.as_deref())?;
                let value = V::from_nullable_sql(value.as_deref())?;
                Ok((key, value))
            })
            .collect()
    }
}

impl<KST, VST, K, V> Queryable<Map<KST, VST>, ClickHouse> for BTreeMap<K, V>
where
    KST: SqlType,
    VST: SqlType,
    Self: FromSql<Map<KST, VST>, ClickHouse>,
{
    type Row = Self;

    fn build(row: Self::Row) -> deserialize::Result<Self> {
        Ok(row)
    }
}

macro_rules! impl_tuple_from_sql {
    ($($rust:ident : $sql:ident),+ $(,)?) => {
        impl<$($sql,)* $($rust,)*> FromSql<Tuple<($($sql,)*)>, ClickHouse> for ($($rust,)*)
        where
            $($sql: SqlType,)*
            $($rust: FromSql<$sql, ClickHouse>,)*
        {
            fn from_sql(
                bytes: <ClickHouse as diesel::backend::Backend>::RawValue<'_>,
            ) -> deserialize::Result<Self> {
                let value = str::from_utf8(bytes)?;
                let mut items = parse_clickhouse_list(value, '(', ')')?.into_iter();
                let tuple = ($({
                    let item = items.next().ok_or_else(|| {
                        format!(
                            "ClickHouse tuple literal had fewer fields than expected: {value:?}"
                        )
                    })?;
                    let item = decode_clickhouse_scalar(item)?;
                    <$rust as FromSql<$sql, ClickHouse>>::from_nullable_sql(item.as_deref())?
                },)*);
                if items.next().is_some() {
                    return Err(format!(
                        "ClickHouse tuple literal had more fields than expected: {value:?}"
                    )
                    .into());
                }
                Ok(tuple)
            }
        }

        impl<$($sql,)* $($rust,)*> Queryable<Tuple<($($sql,)*)>, ClickHouse> for ($($rust,)*)
        where
            ($($sql,)*): SqlType,
            Self: FromSql<Tuple<($($sql,)*)>, ClickHouse>,
        {
            type Row = Self;

            fn build(row: Self::Row) -> deserialize::Result<Self> {
                Ok(row)
            }
        }
    };
}

impl_tuple_from_sql!(T0: ST0);
impl_tuple_from_sql!(T0: ST0, T1: ST1);
impl_tuple_from_sql!(T0: ST0, T1: ST1, T2: ST2);
impl_tuple_from_sql!(T0: ST0, T1: ST1, T2: ST2, T3: ST3);
impl_tuple_from_sql!(T0: ST0, T1: ST1, T2: ST2, T3: ST3, T4: ST4);
impl_tuple_from_sql!(T0: ST0, T1: ST1, T2: ST2, T3: ST3, T4: ST4, T5: ST5);
impl_tuple_from_sql!(T0: ST0, T1: ST1, T2: ST2, T3: ST3, T4: ST4, T5: ST5, T6: ST6);
impl_tuple_from_sql!(T0: ST0, T1: ST1, T2: ST2, T3: ST3, T4: ST4, T5: ST5, T6: ST6, T7: ST7);

fn parse_clickhouse_list(value: &str, open: char, close: char) -> deserialize::Result<Vec<&str>> {
    let value = value.trim();
    let Some(inner) = value.strip_prefix(open).and_then(|v| v.strip_suffix(close)) else {
        return Err(format!("expected ClickHouse {open}{close} literal, got {value:?}").into());
    };
    split_top_level(inner, ',')
}

fn parse_clickhouse_map(value: &str) -> deserialize::Result<Vec<(&str, &str)>> {
    let entries = parse_clickhouse_list(value, '{', '}')?;
    entries
        .into_iter()
        .map(|entry| {
            split_top_level_once(entry, ':').ok_or_else(|| {
                format!("expected ClickHouse map entry key:value, got {entry:?}").into()
            })
        })
        .collect()
}

fn split_top_level(value: &str, delimiter: char) -> deserialize::Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = false;
    let mut escape = false;
    let mut depth = 0_i32;

    for (idx, ch) in value.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if quote {
            match ch {
                '\\' => escape = true,
                '\'' => quote = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' => quote = true,
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ch if ch == delimiter && depth == 0 => {
                parts.push(value[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if quote {
        return Err("unterminated ClickHouse string literal".into());
    }
    if depth != 0 {
        return Err("unbalanced ClickHouse composite literal".into());
    }

    let tail = value[start..].trim();
    if !(tail.is_empty() && parts.is_empty()) {
        parts.push(tail);
    }
    Ok(parts)
}

fn split_top_level_once(value: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut quote = false;
    let mut escape = false;
    let mut depth = 0_i32;

    for (idx, ch) in value.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if quote {
            match ch {
                '\\' => escape = true,
                '\'' => quote = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' => quote = true,
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ch if ch == delimiter && depth == 0 => {
                let right_start = idx + ch.len_utf8();
                return Some((value[..idx].trim(), value[right_start..].trim()));
            }
            _ => {}
        }
    }
    None
}

fn decode_clickhouse_scalar(value: &str) -> deserialize::Result<Option<Vec<u8>>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("NULL") {
        return Ok(None);
    }
    if value.starts_with('\'') {
        return decode_clickhouse_quoted_string(value).map(Some);
    }
    Ok(Some(value.as_bytes().to_vec()))
}

fn decode_clickhouse_quoted_string(value: &str) -> deserialize::Result<Vec<u8>> {
    let mut chars = value.chars();
    if chars.next() != Some('\'') || !value.ends_with('\'') {
        return Err(format!("invalid ClickHouse string literal: {value:?}").into());
    }

    let inner = &value[1..value.len() - 1];
    let mut decoded = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' && matches!(chars.peek(), Some('\'')) {
            chars.next();
            decoded.push('\'');
            continue;
        }
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        match chars.next() {
            Some('0') => decoded.push('\0'),
            Some('b') => decoded.push('\u{0008}'),
            Some('f') => decoded.push('\u{000c}'),
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            Some('t') => decoded.push('\t'),
            Some('\\') => decoded.push('\\'),
            Some('\'') => decoded.push('\''),
            Some(other) => decoded.push(other),
            None => decoded.push('\\'),
        }
    }
    Ok(decoded.into_bytes())
}
