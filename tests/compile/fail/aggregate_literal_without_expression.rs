use diesel_clickhouse::aggregate;

fn main() {
    let _ = aggregate::<diesel::sql_types::Double>("sum").arg(1.0_f64);
}
