//! Serialization support for the lightweight ClickHouse backend.
//!
//! These implementations are deliberately simple: values are written in a
//! textual representation suitable for clients that later bind or inline them.
//! A future `Connection` adapter can replace the bind collector if it needs a
//! native binary format.

use std::error::Error;
use std::io::Write;
use std::str;

use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::{BigInt, Bool, Double, Float, Integer, SmallInt};

use crate::backend::ClickHouse;
use crate::types::{Int8, UInt8, UInt16, UInt32, UInt64};

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
impl_textual_to_sql!(u8, UInt8);
impl_textual_to_sql!(u16, UInt16);
impl_textual_to_sql!(u32, UInt32);
impl_textual_to_sql!(u64, UInt64);
impl_textual_to_sql!(f32, Float);
impl_textual_to_sql!(f64, Double);

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
impl_parse_from_sql!(u8, UInt8);
impl_parse_from_sql!(u16, UInt16);
impl_parse_from_sql!(u32, UInt32);
impl_parse_from_sql!(u64, UInt64);
impl_parse_from_sql!(f32, Float);
impl_parse_from_sql!(f64, Double);
