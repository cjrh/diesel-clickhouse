//! Large binds must not hit the client-side "uri too long" cap.
//!
//! Needs a ClickHouse server. Set `CLICKHOUSE_TEST_URL` (e.g. `http://localhost:8123`),
//! plus `CLICKHOUSE_TEST_USER` / `CLICKHOUSE_TEST_PASSWORD` if needed, and run
//! `cargo test --test large_binds_live -- --ignored`.

use diesel::QueryableByName;
use diesel::sql_types::BigInt;
use diesel_async::RunQueryDsl;
use diesel_clickhouse::AsyncClickHouseConnection;
use diesel_clickhouse::sql_types::{Array, UInt64};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn conn() -> AsyncClickHouseConnection {
    let mut client = clickhouse::Client::default()
        .with_url(std::env::var("CLICKHOUSE_TEST_URL").expect("CLICKHOUSE_TEST_URL"));
    if let Ok(user) = std::env::var("CLICKHOUSE_TEST_USER") {
        client = client
            .with_user(user)
            .with_password(std::env::var("CLICKHOUSE_TEST_PASSWORD").unwrap_or_default());
    }
    AsyncClickHouseConnection::with_client(client)
}

#[tokio::test]
#[ignore]
async fn array_bind_of_30k_ids_runs() {
    let mut conn = conn();
    let ids: Vec<u64> = (0..30_000).collect();
    let rows: Vec<Count> = diesel::sql_query("SELECT toInt64(length(?)) AS n")
        .bind::<Array<UInt64>, _>(ids)
        .load(&mut conn)
        .await
        .expect("a 30k-element array bind must not fail with 'uri too long'");
    assert_eq!(rows[0].n, 30_000);
}

#[tokio::test]
#[ignore]
async fn small_budget_still_answers_correctly() {
    let mut conn = conn().with_max_param_uri_bytes(0);
    let rows: Vec<Count> = diesel::sql_query("SELECT toInt64(arraySum(?)) AS n")
        .bind::<Array<UInt64>, _>(vec![1_u64, 2, 3])
        .load(&mut conn)
        .await
        .expect("inlined, cast array should execute");
    assert_eq!(rows[0].n, 6);
}
