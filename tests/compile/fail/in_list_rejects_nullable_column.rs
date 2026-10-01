use diesel::dsl::sql;
use diesel::sql_types::{BigInt, Nullable};
use diesel_clickhouse::in_list;

fn main() {
    // `Vec<Option<i64>>` loses its nullability in an array bind, so a nullable
    // column must not be accepted.
    let _ = in_list(sql::<Nullable<BigInt>>("x"), vec![None::<i64>]);
}
