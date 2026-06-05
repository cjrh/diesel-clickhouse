//! Typed bindings for common ClickHouse scalar and aggregate functions.
//!
//! The simple cases use Diesel's `define_sql_function!` macro so the generated
//! nodes participate in Diesel's normal expression, grouping and query-fragment
//! machinery.

use diesel::expression::functions::define_sql_function;
use diesel::sql_types::{SingleValue, SqlType};

use crate::types::AggregateFunction;

define_sql_function! {
    /// `toDate(expr)`.
    #[sql_name = "toDate"]
    fn to_date<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Date;
}

define_sql_function! {
    /// `toDateTime(expr)`.
    #[sql_name = "toDateTime"]
    fn to_date_time<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Timestamp;
}

define_sql_function! {
    /// `toDateTime64(expr, scale)`.
    #[sql_name = "toDateTime64"]
    fn to_date_time64<T: SqlType + SingleValue, Scale: SqlType + SingleValue>(expr: T, scale: Scale) -> crate::types::DateTime64;
}

define_sql_function! {
    /// `toStartOfMinute(expr)`.
    #[sql_name = "toStartOfMinute"]
    fn to_start_of_minute<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Timestamp;
}

define_sql_function! {
    /// `toStartOfHour(expr)`.
    #[sql_name = "toStartOfHour"]
    fn to_start_of_hour<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Timestamp;
}

define_sql_function! {
    /// `toStartOfDay(expr)`.
    #[sql_name = "toStartOfDay"]
    fn to_start_of_day<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Date;
}

define_sql_function! {
    /// `toStartOfMonth(expr)`.
    #[sql_name = "toStartOfMonth"]
    fn to_start_of_month<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Date;
}

define_sql_function! {
    /// `toStartOfYear(expr)`.
    #[sql_name = "toStartOfYear"]
    fn to_start_of_year<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Date;
}

define_sql_function! {
    /// `toYear(expr)`.
    #[sql_name = "toYear"]
    fn to_year<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Integer;
}

define_sql_function! {
    /// `toMonth(expr)`.
    #[sql_name = "toMonth"]
    fn to_month<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Integer;
}

define_sql_function! {
    /// `toDayOfMonth(expr)`.
    #[sql_name = "toDayOfMonth"]
    fn to_day_of_month<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Integer;
}

define_sql_function! {
    /// `toHour(expr)`.
    #[sql_name = "toHour"]
    fn to_hour<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Integer;
}

define_sql_function! {
    /// `toMinute(expr)`.
    #[sql_name = "toMinute"]
    fn to_minute<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Integer;
}

define_sql_function! {
    /// `toUnixTimestamp(expr)`.
    #[sql_name = "toUnixTimestamp"]
    fn to_unix_timestamp<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `dateDiff(unit, start, end)`.
    #[sql_name = "dateDiff"]
    fn date_diff<Start: SqlType + SingleValue, End: SqlType + SingleValue>(unit: diesel::sql_types::Text, start: Start, end: End) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `dateTrunc(unit, expr)`.
    #[sql_name = "dateTrunc"]
    fn date_trunc<T: SqlType + SingleValue>(unit: diesel::sql_types::Text, expr: T) -> diesel::sql_types::Timestamp;
}

define_sql_function! {
    /// `intDiv(left, right)`.
    #[sql_name = "intDiv"]
    fn int_div<L: SqlType + SingleValue, R: SqlType + SingleValue>(left: L, right: R) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `toBool(expr)`.
    #[sql_name = "toBool"]
    fn to_bool<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `toInt8(expr)`.
    #[sql_name = "toInt8"]
    fn to_int8<T: SqlType + SingleValue>(expr: T) -> crate::types::Int8;
}

define_sql_function! {
    /// `toInt16(expr)`.
    #[sql_name = "toInt16"]
    fn to_int16<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::SmallInt;
}

define_sql_function! {
    /// `toInt32(expr)`.
    #[sql_name = "toInt32"]
    fn to_int32<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Integer;
}

define_sql_function! {
    /// `toInt64(expr)`.
    #[sql_name = "toInt64"]
    fn to_int64<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `toInt128(expr)`.
    #[sql_name = "toInt128"]
    fn to_int128<T: SqlType + SingleValue>(expr: T) -> crate::types::Int128;
}

define_sql_function! {
    /// `toInt256(expr)`.
    #[sql_name = "toInt256"]
    fn to_int256<T: SqlType + SingleValue>(expr: T) -> crate::types::Int256;
}

define_sql_function! {
    /// `toInt32OrNull(expr)`.
    #[sql_name = "toInt32OrNull"]
    fn to_int32_or_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Nullable<diesel::sql_types::Integer>;
}

define_sql_function! {
    /// `toInt64OrNull(expr)`.
    #[sql_name = "toInt64OrNull"]
    fn to_int64_or_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Nullable<diesel::sql_types::BigInt>;
}

define_sql_function! {
    /// `toInt64OrZero(expr)`.
    #[sql_name = "toInt64OrZero"]
    fn to_int64_or_zero<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `toUInt8(expr)`.
    #[sql_name = "toUInt8"]
    fn to_uint8<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt8;
}

define_sql_function! {
    /// `toUInt16(expr)`.
    #[sql_name = "toUInt16"]
    fn to_uint16<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt16;
}

define_sql_function! {
    /// `toUInt32(expr)`.
    #[sql_name = "toUInt32"]
    fn to_uint32<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt32;
}

define_sql_function! {
    /// `toUInt64(expr)`.
    #[sql_name = "toUInt64"]
    fn to_uint64<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt64;
}

define_sql_function! {
    /// `toUInt128(expr)`.
    #[sql_name = "toUInt128"]
    fn to_uint128<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt128;
}

define_sql_function! {
    /// `toUInt256(expr)`.
    #[sql_name = "toUInt256"]
    fn to_uint256<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt256;
}

define_sql_function! {
    /// `toUInt32OrNull(expr)`.
    #[sql_name = "toUInt32OrNull"]
    fn to_uint32_or_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Nullable<crate::types::UInt32>;
}

define_sql_function! {
    /// `toUInt64OrNull(expr)`.
    #[sql_name = "toUInt64OrNull"]
    fn to_uint64_or_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Nullable<crate::types::UInt64>;
}

define_sql_function! {
    /// `toUInt64OrZero(expr)`.
    #[sql_name = "toUInt64OrZero"]
    fn to_uint64_or_zero<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt64;
}

define_sql_function! {
    /// `toFloat32(expr)`.
    #[sql_name = "toFloat32"]
    fn to_float32<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Float;
}

define_sql_function! {
    /// `toFloat64(expr)`.
    #[sql_name = "toFloat64"]
    fn to_float64<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `toFloat64OrNull(expr)`.
    #[sql_name = "toFloat64OrNull"]
    fn to_float64_or_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Nullable<diesel::sql_types::Double>;
}

define_sql_function! {
    /// `toFloat64OrZero(expr)`.
    #[sql_name = "toFloat64OrZero"]
    fn to_float64_or_zero<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `toString(expr)`.
    #[sql_name = "toString"]
    fn to_string<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `abs(expr)`.
    #[sql_name = "abs"]
    fn abs<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `round(expr)`.
    #[sql_name = "round"]
    fn round<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `floor(expr)`.
    #[sql_name = "floor"]
    fn floor<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `ceil(expr)`.
    #[sql_name = "ceil"]
    fn ceil<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `least(left, right)`.
    #[sql_name = "least"]
    fn least<T: SqlType + SingleValue>(left: T, right: T) -> T;
}

define_sql_function! {
    /// `greatest(left, right)`.
    #[sql_name = "greatest"]
    fn greatest<T: SqlType + SingleValue>(left: T, right: T) -> T;
}

define_sql_function! {
    /// `if(cond, then_expr, else_expr)`.
    #[sql_name = "if"]
    fn if_<Cond: SqlType + SingleValue, T: SqlType + SingleValue>(cond: Cond, then_expr: T, else_expr: T) -> T;
}

define_sql_function! {
    /// `length(expr)`.
    #[sql_name = "length"]
    fn length<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `lengthUTF8(expr)`.
    #[sql_name = "lengthUTF8"]
    fn length_utf8<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `empty(expr)`.
    #[sql_name = "empty"]
    fn empty<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `notEmpty(expr)`.
    #[sql_name = "notEmpty"]
    fn not_empty<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `isNull(expr)`.
    #[sql_name = "isNull"]
    fn is_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `isNotNull(expr)`.
    #[sql_name = "isNotNull"]
    fn is_not_null<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `lower(expr)`.
    #[sql_name = "lower"]
    fn lower<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `upper(expr)`.
    #[sql_name = "upper"]
    fn upper<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `substring(expr, offset, length)`.
    #[sql_name = "substring"]
    fn substring<T: SqlType + SingleValue>(expr: T, offset: diesel::sql_types::BigInt, length: diesel::sql_types::BigInt) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `leftUTF8(expr, length)`.
    #[sql_name = "leftUTF8"]
    fn left_utf8<T: SqlType + SingleValue>(expr: T, length: diesel::sql_types::BigInt) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `position(haystack, needle)`.
    #[sql_name = "position"]
    fn position<T: SqlType + SingleValue>(haystack: T, needle: diesel::sql_types::Text) -> crate::types::UInt64;
}

define_sql_function! {
    /// `positionCaseInsensitive(haystack, needle)`.
    #[sql_name = "positionCaseInsensitive"]
    fn position_case_insensitive<T: SqlType + SingleValue>(haystack: T, needle: diesel::sql_types::Text) -> crate::types::UInt64;
}

define_sql_function! {
    /// `replaceAll(haystack, pattern, replacement)`.
    #[sql_name = "replaceAll"]
    fn replace_all<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text, replacement: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `concat(left, right)`.
    #[sql_name = "concat"]
    fn concat<L: SqlType + SingleValue>(left: L, right: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `nullIf(expr, null_value)`.
    #[sql_name = "nullIf"]
    fn null_if<T: SqlType + SingleValue>(expr: T, null_value: T) -> diesel::sql_types::Nullable<T>;
}

define_sql_function! {
    /// `match(haystack, pattern)`.
    #[sql_name = "match"]
    fn regexp_match<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `like(haystack, pattern)` function form of `haystack LIKE pattern`.
    #[sql_name = "like"]
    fn like<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `like(haystack, pattern, escape)` with a custom escape character.
    #[sql_name = "like"]
    fn like_escape<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text, escape: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `notLike(haystack, pattern)` function form of `haystack NOT LIKE pattern`.
    #[sql_name = "notLike"]
    fn not_like<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `notLike(haystack, pattern, escape)` with a custom escape character.
    #[sql_name = "notLike"]
    fn not_like_escape<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text, escape: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `ilike(haystack, pattern)` function form of `haystack ILIKE pattern`.
    #[sql_name = "ilike"]
    fn ilike<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `ilike(haystack, pattern, escape)` with a custom escape character.
    #[sql_name = "ilike"]
    fn ilike_escape<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text, escape: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `notILike(haystack, pattern)` function form of `haystack NOT ILIKE pattern`.
    #[sql_name = "notILike"]
    fn not_ilike<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `notILike(haystack, pattern, escape)` with a custom escape character.
    #[sql_name = "notILike"]
    fn not_ilike_escape<T: SqlType + SingleValue>(haystack: T, pattern: diesel::sql_types::Text, escape: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `multiMatchAny(haystack, patterns)`.
    #[sql_name = "multiMatchAny"]
    fn multi_match_any<T: SqlType + SingleValue>(haystack: T, patterns: crate::types::Array<diesel::sql_types::Text>) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `multiMatchAnyIndex(haystack, patterns)`.
    #[sql_name = "multiMatchAnyIndex"]
    fn multi_match_any_index<T: SqlType + SingleValue>(haystack: T, patterns: crate::types::Array<diesel::sql_types::Text>) -> crate::types::UInt64;
}

define_sql_function! {
    /// `multiMatchAllIndices(haystack, patterns)`.
    #[sql_name = "multiMatchAllIndices"]
    fn multi_match_all_indices<T: SqlType + SingleValue>(haystack: T, patterns: crate::types::Array<diesel::sql_types::Text>) -> crate::types::Array<crate::types::UInt64>;
}

define_sql_function! {
    /// `multiFuzzyMatchAny(haystack, distance, patterns)`.
    #[sql_name = "multiFuzzyMatchAny"]
    fn multi_fuzzy_match_any<T: SqlType + SingleValue>(haystack: T, distance: diesel::sql_types::Integer, patterns: crate::types::Array<diesel::sql_types::Text>) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `multiFuzzyMatchAnyIndex(haystack, distance, patterns)`.
    #[sql_name = "multiFuzzyMatchAnyIndex"]
    fn multi_fuzzy_match_any_index<T: SqlType + SingleValue>(haystack: T, distance: diesel::sql_types::Integer, patterns: crate::types::Array<diesel::sql_types::Text>) -> crate::types::UInt64;
}

define_sql_function! {
    /// `multiFuzzyMatchAllIndices(haystack, distance, patterns)`.
    #[sql_name = "multiFuzzyMatchAllIndices"]
    fn multi_fuzzy_match_all_indices<T: SqlType + SingleValue>(haystack: T, distance: diesel::sql_types::Integer, patterns: crate::types::Array<diesel::sql_types::Text>) -> crate::types::Array<crate::types::UInt64>;
}

define_sql_function! {
    /// `has(array, value)`.
    #[sql_name = "has"]
    fn has<T: SqlType + SingleValue>(array: crate::types::Array<T>, value: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `hasAny(left, right)`.
    #[sql_name = "hasAny"]
    fn has_any<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `hasAll(left, right)`.
    #[sql_name = "hasAll"]
    fn has_all<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `arrayJoin(array)` — expression form of ClickHouse ARRAY JOIN.
    #[sql_name = "arrayJoin"]
    fn array_join<T: SqlType + SingleValue>(array: crate::types::Array<T>) -> T;
}

define_sql_function! {
    /// `arrayElement(array, index)`.
    #[sql_name = "arrayElement"]
    fn array_element<T: SqlType + SingleValue, Index: SqlType + SingleValue>(array: crate::types::Array<T>, index: Index) -> T;
}

define_sql_function! {
    /// `arrayConcat(left, right)`.
    #[sql_name = "arrayConcat"]
    fn array_concat<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> crate::types::Array<T>;
}

define_sql_function! {
    /// `arrayDistinct(array)`.
    #[sql_name = "arrayDistinct"]
    fn array_distinct<T: SqlType + SingleValue>(array: crate::types::Array<T>) -> crate::types::Array<T>;
}

define_sql_function! {
    /// `mapKeys(map)`.
    #[sql_name = "mapKeys"]
    fn map_keys<K: SqlType + SingleValue, V: SqlType + SingleValue>(map: crate::types::Map<K, V>) -> crate::types::Array<K>;
}

define_sql_function! {
    /// `mapValues(map)`.
    #[sql_name = "mapValues"]
    fn map_values<K: SqlType + SingleValue, V: SqlType + SingleValue>(map: crate::types::Map<K, V>) -> crate::types::Array<V>;
}

define_sql_function! {
    /// `mapContains(map, key)`.
    #[sql_name = "mapContains"]
    fn map_contains<K: SqlType + SingleValue, V: SqlType + SingleValue>(map: crate::types::Map<K, V>, key: K) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `mapFromArrays(keys, values)`.
    #[sql_name = "mapFromArrays"]
    fn map_from_arrays<K: SqlType + SingleValue, V: SqlType + SingleValue>(keys: crate::types::Array<K>, values: crate::types::Array<V>) -> crate::types::Map<K, V>;
}

define_sql_function! {
    /// `JSONExtractString(json, key)`.
    #[sql_name = "JSONExtractString"]
    fn json_extract_string<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `JSONExtractInt(json, key)`.
    #[sql_name = "JSONExtractInt"]
    fn json_extract_int<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `JSONExtractFloat(json, key)`.
    #[sql_name = "JSONExtractFloat"]
    fn json_extract_float<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `JSONExtractBool(json, key)`.
    #[sql_name = "JSONExtractBool"]
    fn json_extract_bool<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `JSONExtractRaw(json, key)`.
    #[sql_name = "JSONExtractRaw"]
    fn json_extract_raw<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `JSONExtractUInt(json, key)`.
    #[sql_name = "JSONExtractUInt"]
    fn json_extract_uint<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> crate::types::UInt64;
}

define_sql_function! {
    /// `JSONExtractStringCaseInsensitive(json, key)`.
    #[sql_name = "JSONExtractStringCaseInsensitive"]
    fn json_extract_string_ci<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `JSONExtractIntCaseInsensitive(json, key)`.
    #[sql_name = "JSONExtractIntCaseInsensitive"]
    fn json_extract_int_ci<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `JSONExtractRawCaseInsensitive(json, key)`.
    #[sql_name = "JSONExtractRawCaseInsensitive"]
    fn json_extract_raw_ci<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `JSONHas(json, key)`.
    #[sql_name = "JSONHas"]
    fn json_has<Json: SqlType + SingleValue>(json: Json, key: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `JSONLength(json)`.
    #[sql_name = "JSONLength"]
    fn json_length<Json: SqlType + SingleValue>(json: Json) -> crate::types::UInt64;
}

define_sql_function! {
    /// `JSON_VALUE(json, path)`.
    #[sql_name = "JSON_VALUE"]
    fn json_value<Json: SqlType + SingleValue>(json: Json, path: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `JSON_QUERY(json, path)`.
    #[sql_name = "JSON_QUERY"]
    fn json_query<Json: SqlType + SingleValue>(json: Json, path: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `JSON_EXISTS(json, path)`.
    #[sql_name = "JSON_EXISTS"]
    fn json_exists<Json: SqlType + SingleValue>(json: Json, path: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `isValidJSON(json)`.
    #[sql_name = "isValidJSON"]
    fn is_valid_json<Json: SqlType + SingleValue>(json: Json) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `simpleJSONExtractString(json, field_name)`.
    #[sql_name = "simpleJSONExtractString"]
    fn simple_json_extract_string<Json: SqlType + SingleValue>(json: Json, field_name: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `simpleJSONExtractInt(json, field_name)`.
    #[sql_name = "simpleJSONExtractInt"]
    fn simple_json_extract_int<Json: SqlType + SingleValue>(json: Json, field_name: diesel::sql_types::Text) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `simpleJSONExtractFloat(json, field_name)`.
    #[sql_name = "simpleJSONExtractFloat"]
    fn simple_json_extract_float<Json: SqlType + SingleValue>(json: Json, field_name: diesel::sql_types::Text) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `simpleJSONHas(json, field_name)`.
    #[sql_name = "simpleJSONHas"]
    fn simple_json_has<Json: SqlType + SingleValue>(json: Json, field_name: diesel::sql_types::Text) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `domain(url)`.
    #[sql_name = "domain"]
    fn domain<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `domainWithoutWWW(url)`.
    #[sql_name = "domainWithoutWWW"]
    fn domain_without_www<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `topLevelDomain(url)`.
    #[sql_name = "topLevelDomain"]
    fn top_level_domain<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `firstSignificantSubdomain(url)`.
    #[sql_name = "firstSignificantSubdomain"]
    fn first_significant_subdomain<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `path(url)`.
    #[sql_name = "path"]
    fn url_path<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `pathFull(url)`.
    #[sql_name = "pathFull"]
    fn url_path_full<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `queryString(url)`.
    #[sql_name = "queryString"]
    fn url_query_string<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `fragment(url)`.
    #[sql_name = "fragment"]
    fn url_fragment<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `protocol(url)`.
    #[sql_name = "protocol"]
    fn url_protocol<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `cutQueryString(url)`.
    #[sql_name = "cutQueryString"]
    fn cut_query_string<T: SqlType + SingleValue>(url: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `hex(expr)`.
    #[sql_name = "hex"]
    fn hex<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `unhex(expr)`.
    #[sql_name = "unhex"]
    fn unhex<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `base64Encode(expr)`.
    #[sql_name = "base64Encode"]
    fn base64_encode<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `base64Decode(expr)`.
    #[sql_name = "base64Decode"]
    fn base64_decode<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `tryBase64Decode(expr)`.
    #[sql_name = "tryBase64Decode"]
    fn try_base64_decode<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Nullable<diesel::sql_types::Text>;
}

define_sql_function! {
    /// `cityHash64(expr)`.
    #[sql_name = "cityHash64"]
    fn city_hash64<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt64;
}

define_sql_function! {
    /// `sipHash64(expr)`.
    #[sql_name = "sipHash64"]
    fn sip_hash64<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt64;
}

define_sql_function! {
    /// `xxHash64(expr)`.
    #[sql_name = "xxHash64"]
    fn xx_hash64<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt64;
}

define_sql_function! {
    /// `farmFingerprint64(expr)`.
    #[sql_name = "farmFingerprint64"]
    fn farm_fingerprint64<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt64;
}

define_sql_function! {
    /// `toIPv4(expr)`.
    #[sql_name = "toIPv4"]
    fn to_ipv4<T: SqlType + SingleValue>(expr: T) -> crate::types::IPv4;
}

define_sql_function! {
    /// `toIPv6(expr)`.
    #[sql_name = "toIPv6"]
    fn to_ipv6<T: SqlType + SingleValue>(expr: T) -> crate::types::IPv6;
}

define_sql_function! {
    /// `IPv4StringToNum(expr)`.
    #[sql_name = "IPv4StringToNum"]
    fn ipv4_string_to_num<T: SqlType + SingleValue>(expr: T) -> crate::types::UInt32;
}

define_sql_function! {
    /// `IPv4NumToString(expr)`.
    #[sql_name = "IPv4NumToString"]
    fn ipv4_num_to_string<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `IPv6NumToString(expr)`.
    #[sql_name = "IPv6NumToString"]
    fn ipv6_num_to_string<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Text;
}

define_sql_function! {
    /// `isIPv4String(expr)`.
    #[sql_name = "isIPv4String"]
    fn is_ipv4_string<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `isIPv6String(expr)`.
    #[sql_name = "isIPv6String"]
    fn is_ipv6_string<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Bool;
}

define_sql_function! {
    /// `L1Distance(vector1, vector2)`.
    #[sql_name = "L1Distance"]
    fn l1_distance<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `L2Distance(vector1, vector2)`.
    #[sql_name = "L2Distance"]
    fn l2_distance<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `LinfDistance(vector1, vector2)`.
    #[sql_name = "LinfDistance"]
    fn linf_distance<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `cosineDistance(vector1, vector2)`.
    #[sql_name = "cosineDistance"]
    fn cosine_distance<T: SqlType + SingleValue>(left: crate::types::Array<T>, right: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `L1Norm(vector)`.
    #[sql_name = "L1Norm"]
    fn l1_norm<T: SqlType + SingleValue>(vector: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `L2Norm(vector)`.
    #[sql_name = "L2Norm"]
    fn l2_norm<T: SqlType + SingleValue>(vector: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `LinfNorm(vector)`.
    #[sql_name = "LinfNorm"]
    fn linf_norm<T: SqlType + SingleValue>(vector: crate::types::Array<T>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `countState()`.
    #[aggregate]
    #[sql_name = "countState"]
    fn count_state() -> AggregateFunction<diesel::sql_types::BigInt>;
}

define_sql_function! {
    /// `countMerge(state)`.
    #[aggregate]
    #[sql_name = "countMerge"]
    fn count_merge(state: AggregateFunction<diesel::sql_types::BigInt>) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `countMergeState(state)`.
    #[aggregate]
    #[sql_name = "countMergeState"]
    fn count_merge_state(state: AggregateFunction<diesel::sql_types::BigInt>) -> AggregateFunction<diesel::sql_types::BigInt>;
}

define_sql_function! {
    /// `sumState(expr)`.
    #[aggregate]
    #[sql_name = "sumState"]
    fn sum_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<T>;
}

define_sql_function! {
    /// `sumMerge(state)`.
    #[aggregate]
    #[sql_name = "sumMerge"]
    fn sum_merge<T: SqlType + SingleValue>(state: AggregateFunction<T>) -> T;
}

define_sql_function! {
    /// `sumMergeState(state)`.
    #[aggregate]
    #[sql_name = "sumMergeState"]
    fn sum_merge_state<T: SqlType + SingleValue>(state: AggregateFunction<T>) -> AggregateFunction<T>;
}

define_sql_function! {
    /// `avgState(expr)`.
    #[aggregate]
    #[sql_name = "avgState"]
    fn avg_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<diesel::sql_types::Double>;
}

define_sql_function! {
    /// `avgMerge(state)`.
    #[aggregate]
    #[sql_name = "avgMerge"]
    fn avg_merge(state: AggregateFunction<diesel::sql_types::Double>) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `avgMergeState(state)`.
    #[aggregate]
    #[sql_name = "avgMergeState"]
    fn avg_merge_state(state: AggregateFunction<diesel::sql_types::Double>) -> AggregateFunction<diesel::sql_types::Double>;
}

define_sql_function! {
    /// `minState(expr)`.
    #[aggregate]
    #[sql_name = "minState"]
    fn min_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<T>;
}

define_sql_function! {
    /// `minMerge(state)`.
    #[aggregate]
    #[sql_name = "minMerge"]
    fn min_merge<T: SqlType + SingleValue>(state: AggregateFunction<T>) -> T;
}

define_sql_function! {
    /// `maxState(expr)`.
    #[aggregate]
    #[sql_name = "maxState"]
    fn max_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<T>;
}

define_sql_function! {
    /// `maxMerge(state)`.
    #[aggregate]
    #[sql_name = "maxMerge"]
    fn max_merge<T: SqlType + SingleValue>(state: AggregateFunction<T>) -> T;
}

define_sql_function! {
    /// `uniqState(expr)`.
    #[aggregate]
    #[sql_name = "uniqState"]
    fn uniq_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<diesel::sql_types::BigInt>;
}

define_sql_function! {
    /// `uniqMerge(state)`.
    #[aggregate]
    #[sql_name = "uniqMerge"]
    fn uniq_merge(state: AggregateFunction<diesel::sql_types::BigInt>) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `uniqExactState(expr)`.
    #[aggregate]
    #[sql_name = "uniqExactState"]
    fn uniq_exact_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<diesel::sql_types::BigInt>;
}

define_sql_function! {
    /// `uniqExactMerge(state)`.
    #[aggregate]
    #[sql_name = "uniqExactMerge"]
    fn uniq_exact_merge(state: AggregateFunction<diesel::sql_types::BigInt>) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `groupArrayState(expr)`.
    #[aggregate]
    #[sql_name = "groupArrayState"]
    fn group_array_state<T: SqlType + SingleValue>(expr: T) -> AggregateFunction<crate::types::Array<T>>;
}

define_sql_function! {
    /// `groupArrayMerge(state)`.
    #[aggregate]
    #[sql_name = "groupArrayMerge"]
    fn group_array_merge<T: SqlType + SingleValue>(state: AggregateFunction<crate::types::Array<T>>) -> crate::types::Array<T>;
}

define_sql_function! {
    /// `finalizeAggregation(state)`.
    #[sql_name = "finalizeAggregation"]
    fn finalize_aggregation<T: SqlType + SingleValue>(state: AggregateFunction<T>) -> T;
}

define_sql_function! {
    /// `count()` — number of rows, returned as ClickHouse's native `UInt64`.
    ///
    /// Prefer this over `aggregate::<UInt64>("count").no_args()` or Diesel's
    /// built-in [`count_star`](diesel::dsl::count_star()). ClickHouse's `count()`
    /// yields `UInt64`, whereas `count_star` is typed `BigInt`; this helper makes
    /// the loaded Rust type (`u64`) match what the server actually sends.
    #[aggregate]
    #[sql_name = "count"]
    fn count() -> crate::types::UInt64;
}

define_sql_function! {
    /// `countIf(predicate)`.
    #[aggregate]
    #[sql_name = "countIf"]
    fn count_if<Cond: SqlType + SingleValue>(cond: Cond) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `sumIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "sumIf"]
    fn sum_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> T;
}

define_sql_function! {
    /// `avgIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "avgIf"]
    fn avg_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `minIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "minIf"]
    fn min_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> T;
}

define_sql_function! {
    /// `maxIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "maxIf"]
    fn max_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> T;
}

define_sql_function! {
    /// `uniq(expr)`.
    #[aggregate]
    #[sql_name = "uniq"]
    fn uniq<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `uniqExact(expr)`.
    #[aggregate]
    #[sql_name = "uniqExact"]
    fn uniq_exact<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `uniqIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "uniqIf"]
    fn uniq_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `uniqExactIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "uniqExactIf"]
    fn uniq_exact_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> diesel::sql_types::BigInt;
}

define_sql_function! {
    /// `groupArray(expr)`.
    #[aggregate]
    #[sql_name = "groupArray"]
    fn group_array<T: SqlType + SingleValue>(expr: T) -> crate::types::Array<T>;
}

define_sql_function! {
    /// `groupArrayIf(expr, predicate)`.
    #[aggregate]
    #[sql_name = "groupArrayIf"]
    fn group_array_if<T: SqlType + SingleValue, Cond: SqlType + SingleValue>(expr: T, cond: Cond) -> crate::types::Array<T>;
}

define_sql_function! {
    /// `any(expr)`.
    #[aggregate]
    #[sql_name = "any"]
    fn any_value<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `anyLast(expr)`.
    #[aggregate]
    #[sql_name = "anyLast"]
    fn any_last<T: SqlType + SingleValue>(expr: T) -> T;
}

define_sql_function! {
    /// `argMax(arg, val)`.
    #[aggregate]
    #[sql_name = "argMax"]
    fn arg_max<T: SqlType + SingleValue, V: SqlType + SingleValue>(arg: T, val: V) -> T;
}

define_sql_function! {
    /// `argMin(arg, val)`.
    #[aggregate]
    #[sql_name = "argMin"]
    fn arg_min<T: SqlType + SingleValue, V: SqlType + SingleValue>(arg: T, val: V) -> T;
}

define_sql_function! {
    /// `stddevPop(x)`.
    #[aggregate]
    #[sql_name = "stddevPop"]
    fn stddev_pop<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `stddevSamp(x)`.
    #[aggregate]
    #[sql_name = "stddevSamp"]
    fn stddev_samp<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `stddevPopStable(x)`.
    #[aggregate]
    #[sql_name = "stddevPopStable"]
    fn stddev_pop_stable<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `stddevSampStable(x)`.
    #[aggregate]
    #[sql_name = "stddevSampStable"]
    fn stddev_samp_stable<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `varPop(x)`.
    #[aggregate]
    #[sql_name = "varPop"]
    fn var_pop<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `varSamp(x)`.
    #[aggregate]
    #[sql_name = "varSamp"]
    fn var_samp<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `varPopStable(x)`.
    #[aggregate]
    #[sql_name = "varPopStable"]
    fn var_pop_stable<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `varSampStable(x)`.
    #[aggregate]
    #[sql_name = "varSampStable"]
    fn var_samp_stable<T: SqlType + SingleValue>(expr: T) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `analysisOfVariance(value, group_no)`.
    #[aggregate]
    #[sql_name = "analysisOfVariance"]
    fn analysis_of_variance<T: SqlType + SingleValue, G: SqlType + SingleValue>(value: T, group_no: G) -> crate::types::Tuple<(diesel::sql_types::Double, diesel::sql_types::Double)>;
}

define_sql_function! {
    /// `mannWhitneyUTest(sample_data, sample_index)`.
    #[aggregate]
    #[sql_name = "mannWhitneyUTest"]
    fn mann_whitney_u_test<T: SqlType + SingleValue, G: SqlType + SingleValue>(sample_data: T, sample_index: G) -> crate::types::Tuple<(diesel::sql_types::Double, diesel::sql_types::Double)>;
}

define_sql_function! {
    /// `corr(x, y)`.
    #[aggregate]
    #[sql_name = "corr"]
    fn corr<X: SqlType + SingleValue, Y: SqlType + SingleValue>(x: X, y: Y) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `covarPop(x, y)`.
    #[aggregate]
    #[sql_name = "covarPop"]
    fn covar_pop<X: SqlType + SingleValue, Y: SqlType + SingleValue>(x: X, y: Y) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `covarSamp(x, y)`.
    #[aggregate]
    #[sql_name = "covarSamp"]
    fn covar_samp<X: SqlType + SingleValue, Y: SqlType + SingleValue>(x: X, y: Y) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `covarPopStable(x, y)`.
    #[aggregate]
    #[sql_name = "covarPopStable"]
    fn covar_pop_stable<X: SqlType + SingleValue, Y: SqlType + SingleValue>(x: X, y: Y) -> diesel::sql_types::Double;
}

define_sql_function! {
    /// `covarSampStable(x, y)`.
    #[aggregate]
    #[sql_name = "covarSampStable"]
    fn covar_samp_stable<X: SqlType + SingleValue, Y: SqlType + SingleValue>(x: X, y: Y) -> diesel::sql_types::Double;
}
