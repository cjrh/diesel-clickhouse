# NYC taxi tutorial with Diesel and ClickHouse

This tutorial mirrors the [ClickHouse NYC taxi tutorial](https://clickhouse.com/docs/tutorial), but shows the Diesel code next to the ClickHouse SQL. It uses the HTTP-backed `ClickHouseConnection` provided by this crate.

The companion executable example can run the same flow against a live ClickHouse server and emit a Markdown report with observed results:

```bash
CLICKHOUSE_URL=http://default:password@localhost:8123/default \
  cargo run --example tutorial -- --write docs/TUTORIAL.md
```

The example downloads the public NYC taxi data from S3, so expect a larger network transfer and a few minutes of runtime on a fresh database.

## Connect

ClickHouse SQL clients connect to the server directly. In Diesel, create a `ClickHouseConnection` from a URL or explicit options:

```rust
use diesel::prelude::*;
use diesel_clickhouse::{ClickHouseConnectionOptions, ClickHouseQueryDsl};

let mut conn = ClickHouseConnectionOptions::new("http://localhost:8123")
    .user("default")
    .password("password")
    .database("default")
    .option("max_threads", "1")
    .connect()?;
```

`ClickHouseConnection::establish("http://user:password@host:8123/database")` is equivalent for URL-first configuration.

## Declare the Diesel schema

Diesel's `table!` schema can declare just the columns your Rust queries use. The ClickHouse table created below has more columns than this compact schema; the example keeps only the columns used in the tutorial queries.

```rust
diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    trips (trip_id) {
        trip_id -> UInt32,
        pickup_date -> Date,
        passenger_count -> UInt8,
        tip_amount -> Float,
        total_amount -> Float,
        pickup_ntaname -> Text,
        dropoff_nyct2010_gid -> UInt8,
    }
}
```

Use `diesel::dsl::sql::<ST>(...)` for tutorial expressions that are ClickHouse-specific but still have a known result type.

## Create the `trips` table

ClickHouse SQL from the tutorial:

```sql
CREATE TABLE trips
(
    `trip_id` UInt32,
    `vendor_id` Enum8('1' = 1, '2' = 2, '3' = 3, '4' = 4, 'CMT' = 5, 'VTS' = 6, 'DDS' = 7, 'B02512' = 10, 'B02598' = 11, 'B02617' = 12, 'B02682' = 13, 'B02764' = 14, '' = 15),
    `pickup_date` Date,
    `pickup_datetime` DateTime,
    `dropoff_date` Date,
    `dropoff_datetime` DateTime,
    `store_and_fwd_flag` UInt8,
    `rate_code_id` UInt8,
    `pickup_longitude` Float64,
    `pickup_latitude` Float64,
    `dropoff_longitude` Float64,
    `dropoff_latitude` Float64,
    `passenger_count` UInt8,
    `trip_distance` Float64,
    `fare_amount` Float32,
    `extra` Float32,
    `mta_tax` Float32,
    `tip_amount` Float32,
    `tolls_amount` Float32,
    `ehail_fee` Float32,
    `improvement_surcharge` Float32,
    `total_amount` Float32,
    `payment_type` Enum8('UNK' = 0, 'CSH' = 1, 'CRE' = 2, 'NOC' = 3, 'DIS' = 4),
    `trip_type` UInt8,
    `pickup` FixedString(25),
    `dropoff` FixedString(25),
    `cab_type` Enum8('yellow' = 1, 'green' = 2, 'uber' = 3),
    `pickup_nyct2010_gid` Int8,
    `pickup_ctlabel` Float32,
    `pickup_borocode` Int8,
    `pickup_ct2010` String,
    `pickup_boroct2010` String,
    `pickup_cdeligibil` String,
    `pickup_ntacode` FixedString(4),
    `pickup_ntaname` String,
    `pickup_puma` UInt16,
    `dropoff_nyct2010_gid` UInt8,
    `dropoff_ctlabel` Float32,
    `dropoff_borocode` UInt8,
    `dropoff_ct2010` String,
    `dropoff_boroct2010` String,
    `dropoff_cdeligibil` String,
    `dropoff_ntacode` FixedString(4),
    `dropoff_ntaname` String,
    `dropoff_puma` UInt16
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(pickup_date)
ORDER BY pickup_datetime;
```

The same table can be built with this crate's DDL builder. `DataType::custom(...)` is the escape hatch for ClickHouse type expressions that do not need a dedicated Rust enum variant, such as `FixedString(25)`.

```rust
use diesel_clickhouse::{create_table, merge_tree, to_sql, DataType};

let ddl = create_table("trips")
    .if_not_exists()
    .column("trip_id", DataType::UInt32)
    .column("vendor_id", DataType::enum8([
        ("1", 1), ("2", 2), ("3", 3), ("4", 4),
        ("CMT", 5), ("VTS", 6), ("DDS", 7),
        ("B02512", 10), ("B02598", 11), ("B02617", 12),
        ("B02682", 13), ("B02764", 14), ("", 15),
    ]))
    .column("pickup_date", DataType::Date)
    .column("pickup_datetime", DataType::DateTime)
    // Add the remaining columns in the same shape. See examples/tutorial.rs.
    .column("pickup", DataType::custom("FixedString(25)"))
    .column("dropoff", DataType::custom("FixedString(25)"))
    .engine(
        merge_tree()
            .partition_by(["toYYYYMM(pickup_date)"])
            .order_by(["pickup_datetime"]),
    );

diesel::sql_query(to_sql(&ddl)?).execute(&mut conn)?;
```

## Add the dataset

The tutorial loads the public dataset from S3 with ClickHouse's `s3(...)` table function:

```sql
INSERT INTO trips
SELECT * FROM s3(
    'https://datasets-documentation.s3.eu-west-3.amazonaws.com/nyc-taxi/trips_{1..2}.gz',
    'TabSeparatedWithNames', '... full schema omitted ...'
) SETTINGS input_format_try_infer_datetimes = 0;
```

This is a good place to use raw SQL with `batch_execute`, because ingestion from a ClickHouse table function is already ClickHouse-specific:

```rust
use diesel::connection::SimpleConnection;

conn.batch_execute(INSERT_TRIPS_SQL)?;

let row_count: i64 = trips::table
    .select(diesel::dsl::count_star())
    .first(&mut conn)?;
assert_eq!(row_count, 1_999_657);
```

`examples/tutorial.rs` contains the full `INSERT_TRIPS_SQL` string from the ClickHouse tutorial.

## Analyze the data

### Average tip amount

```sql
SELECT round(avg(tip_amount), 2) FROM trips;
```

Diesel version:

```rust
use diesel::dsl::sql;
use diesel::sql_types::Float;

let avg_tip: f32 = trips::table
    .select(sql::<Float>("round(avg(`tip_amount`), 2)"))
    .first(&mut conn)?;
```

### Average cost by passenger count

```sql
SELECT
    passenger_count,
    ceil(avg(total_amount), 2) AS average_total_amount
FROM trips
GROUP BY passenger_count;
```

Diesel version:

```rust
let by_passenger: Vec<(u8, f32)> = trips::table
    .group_by(trips::passenger_count)
    .select((
        trips::passenger_count,
        sql::<Float>("ceil(avg(`total_amount`), 2)"),
    ))
    .order(trips::passenger_count.asc())
    .load(&mut conn)?;
```

### Daily pickups per neighborhood

```sql
SELECT
    pickup_date,
    pickup_ntaname,
    SUM(1) AS number_of_trips
FROM trips
GROUP BY pickup_date, pickup_ntaname
ORDER BY pickup_date ASC;
```

Diesel version:

```rust
let daily_pickups: Vec<(String, String, i64)> = trips::table
    .group_by((trips::pickup_date, trips::pickup_ntaname))
    .select((
        trips::pickup_date,
        trips::pickup_ntaname,
        diesel::dsl::count_star(),
    ))
    .order(trips::pickup_date.asc())
    .limit(10)
    .load(&mut conn)?;
```

`Date` currently loads as a string through the HTTP text row decoder, hence `String` for `pickup_date`.

## Create and query a dictionary

The ClickHouse tutorial creates a dictionary from a public CSV:

```sql
CREATE DICTIONARY taxi_zone_dictionary
(
  `LocationID` UInt16 DEFAULT 0,
  `Borough` String,
  `Zone` String,
  `service_zone` String
)
PRIMARY KEY LocationID
SOURCE(HTTP(URL 'https://datasets-documentation.s3.eu-west-3.amazonaws.com/nyc-taxi/taxi_zone_lookup.csv' FORMAT 'CSVWithNames'))
LIFETIME(MIN 0 MAX 0)
LAYOUT(HASHED_ARRAY());
```

Use `batch_execute` for dictionary DDL:

```rust
conn.batch_execute(CREATE_DICTIONARY_SQL)?;
```

Query dictionary functions with typed SQL fragments:

```sql
SELECT dictGet('taxi_zone_dictionary', 'Borough', 132);
SELECT dictHas('taxi_zone_dictionary', 132);
```

```rust
use diesel::sql_types::{Bool, Text};

let borough: String = diesel::select(sql::<Text>(
    "dictGet('taxi_zone_dictionary', 'Borough', 132)",
))
.get_result(&mut conn)?;

let has_jfk: bool = diesel::select(sql::<Bool>(
    "dictHas('taxi_zone_dictionary', 132)",
))
.get_result(&mut conn)?;
```

Use dictionary lookups inside a Diesel query by wrapping the ClickHouse expression in `sql::<Text>(...)` and grouping by the same expression:

```sql
SELECT
    count(1) AS total,
    dictGetOrDefault(
        'taxi_zone_dictionary',
        'Borough',
        toUInt64(pickup_nyct2010_gid),
        'Unknown'
    ) AS borough_name
FROM trips
WHERE dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138
GROUP BY borough_name
ORDER BY total DESC;
```

```rust
fn pickup_borough_expr() -> diesel::expression::SqlLiteral<Text> {
    sql::<Text>(
        "dictGetOrDefault('taxi_zone_dictionary','Borough', \
         toUInt64(`pickup_nyct2010_gid`), 'Unknown')",
    )
}

let borough_pickups: Vec<(i64, String)> = trips::table
    .filter(sql::<Bool>(
        "dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138",
    ))
    .group_by(pickup_borough_expr())
    .select((diesel::dsl::count_star(), pickup_borough_expr()))
    .order(diesel::dsl::count_star().desc())
    .load(&mut conn)?;
```

## Perform a join

ClickHouse can also join the dictionary with the `trips` table:

```sql
SELECT
    count(1) AS total,
    Borough AS borough
FROM trips
JOIN taxi_zone_dictionary
    ON toUInt64(trips.pickup_nyct2010_gid) = taxi_zone_dictionary.LocationID
WHERE dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138
GROUP BY borough
ORDER BY total DESC;
```

Diesel's built-in ANSI join source is not always the shape ClickHouse expects for executable joins, and ClickHouse dictionaries are a particularly SQL-native feature. For this part of the tutorial, use `sql_query` plus `QueryableByName`:

```rust
#[derive(Debug, diesel::QueryableByName)]
struct BoroughTotal {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    total: i64,
    #[diesel(sql_type = diesel::sql_types::Text)]
    borough: String,
}

let joined: Vec<BoroughTotal> = diesel::sql_query(JOIN_SQL)
    .load(&mut conn)?;
```

## What this tutorial demonstrates

- Use Diesel schemas for ClickHouse tables, including declaring only the columns a Rust query needs.
- Build ClickHouse DDL with `create_table`, `DataType`, and `merge_tree`.
- Use `ClickHouseConnection` for ordinary `.load`, `.first`, `.execute`, and `.batch_execute` calls.
- Mix Diesel's typed query builder with `sql::<ST>(...)` for ClickHouse-specific expressions.
- Use `sql_query` / `QueryableByName` when a ClickHouse feature is more naturally represented as SQL than a Diesel AST.
