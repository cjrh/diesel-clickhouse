//! Executable NYC taxi tutorial for `diesel-clickhouse`.
//!
//! The example runs the same broad workflow as the ClickHouse tutorial and can
//! emit a Markdown walkthrough containing the SQL, the Diesel code shape, and
//! the observed results:
//!
//! ```text
//! CLICKHOUSE_URL=http://default:password@localhost:8123/default \
//!   cargo run --example tutorial -- --write docs/TUTORIAL.md
//! ```
//!
//! The run downloads the public NYC taxi data from S3 into a `trips` table.

use std::env;
use std::error::Error;
use std::fs;

use diesel::connection::SimpleConnection;
use diesel::dsl::{count_star, sql};
use diesel::expression::SqlLiteral;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Float, Text};
use diesel_clickhouse::{ClickHouseConnectionOptions, DataType, create_table, merge_tree, to_sql};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    // Diesel schemas may declare only the columns a Rust query needs. The DDL
    // below creates the full ClickHouse tutorial table; this compact schema
    // keeps the example below Diesel's default 32-column compile-time limit.
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

diesel::table! {
    use diesel::sql_types::*;
    use diesel_clickhouse::sql_types::*;

    #[sql_name = "taxi_zone_dictionary"]
    taxi_zone_dictionary (location_id) {
        #[sql_name = "LocationID"]
        location_id -> UInt16,
        #[sql_name = "Borough"]
        borough -> Text,
        #[sql_name = "Zone"]
        zone -> Text,
        service_zone -> Text,
    }
}

#[derive(Debug, QueryableByName)]
struct BoroughTotal {
    #[diesel(sql_type = BigInt)]
    total: i64,
    #[diesel(sql_type = Text)]
    borough: String,
}

fn main() -> Result<()> {
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--write" {
            output = args.next();
        }
    }

    let database_url = env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://default:password@localhost:8123/default".to_owned());
    let mut conn = ClickHouseConnectionOptions::from_url(&database_url)?.connect()?;
    let mut doc = TutorialDoc::new();

    doc.heading(1, "NYC taxi tutorial with Diesel and ClickHouse");
    doc.paragraph("This walkthrough mirrors the ClickHouse NYC taxi tutorial, showing the original ClickHouse SQL next to the Diesel code that renders or executes the same operation. The example binary can regenerate this document with live results from a running ClickHouse server.");
    doc.code("bash", "CLICKHOUSE_URL=http://default:password@localhost:8123/default \\\n  cargo run --example tutorial -- --write docs/TUTORIAL.md");

    doc.heading(2, "Connect");
    doc.rust("use diesel_clickhouse::ClickHouseConnectionOptions;\n\nlet mut conn = ClickHouseConnectionOptions::from_url(\n    \"http://default:password@localhost:8123/default\",\n)?.connect()?;");
    doc.paragraph(&format!("Connected to `{database_url}`."));

    doc.heading(2, "Create the trips table");
    let create_trips_sql = create_trips_sql()?;
    doc.sql(&create_trips_sql);
    doc.rust(CREATE_TRIPS_RUST);
    diesel::sql_query(&create_trips_sql).execute(&mut conn)?;
    doc.output("created `trips` if it did not already exist");

    doc.heading(2, "Load the dataset from S3");
    doc.sql(INSERT_TRIPS_SQL.trim());
    doc.rust("let existing: i64 = trips::table.select(count_star()).first(&mut conn)?;\nif existing == 0 {\n    conn.batch_execute(INSERT_TRIPS_SQL)?;\n}");
    let existing: i64 = trips::table.select(count_star()).first(&mut conn)?;
    if existing == 0 {
        conn.batch_execute(INSERT_TRIPS_SQL)?;
    }
    let row_count: i64 = trips::table.select(count_star()).first(&mut conn)?;
    doc.output(&format!("trips rows: {row_count}"));

    doc.heading(2, "Analyze the data");
    doc.sql("SELECT round(avg(tip_amount), 2) FROM trips");
    doc.rust("let avg_tip: f32 = trips::table\n    .select(sql::<Float>(\"round(avg(`tip_amount`), 2)\"))\n    .first(&mut conn)?;");
    let avg_tip: f32 = trips::table
        .select(sql::<Float>("round(avg(`tip_amount`), 2)"))
        .first(&mut conn)?;
    doc.output(&format!("average tip: {avg_tip:.2}"));

    doc.sql("SELECT passenger_count, ceil(avg(total_amount), 2) AS average_total_amount FROM trips GROUP BY passenger_count");
    doc.rust("let by_passenger: Vec<(u8, f32)> = trips::table\n    .select((passenger_count, sql::<Float>(\"ceil(avg(`total_amount`), 2)\")))\n    .group_by(passenger_count)\n    .order(passenger_count.asc())\n    .load(&mut conn)?;");
    let by_passenger: Vec<(u8, f32)> = trips::table
        .select((
            trips::passenger_count,
            sql::<Float>("ceil(avg(`total_amount`), 2)"),
        ))
        .group_by(trips::passenger_count)
        .order(trips::passenger_count.asc())
        .load(&mut conn)?;
    doc.output(&format_rows(&by_passenger));

    doc.sql("SELECT pickup_date, pickup_ntaname, SUM(1) AS number_of_trips FROM trips GROUP BY pickup_date, pickup_ntaname ORDER BY pickup_date ASC LIMIT 10");
    doc.rust("let daily_pickups: Vec<(String, String, i64)> = trips::table\n    .select((pickup_date, pickup_ntaname, count_star()))\n    .group_by((pickup_date, pickup_ntaname))\n    .order(pickup_date.asc())\n    .limit(10)\n    .load(&mut conn)?;");
    let daily_pickups: Vec<(String, String, i64)> = trips::table
        .group_by((trips::pickup_date, trips::pickup_ntaname))
        .select((trips::pickup_date, trips::pickup_ntaname, count_star()))
        .order(trips::pickup_date.asc())
        .limit(10)
        .load(&mut conn)?;
    doc.output(&format_rows(&daily_pickups));

    doc.heading(2, "Create and query the taxi zone dictionary");
    doc.sql(CREATE_DICTIONARY_SQL.trim());
    doc.rust("conn.batch_execute(CREATE_DICTIONARY_SQL)?;");
    conn.batch_execute("DROP DICTIONARY IF EXISTS taxi_zone_dictionary")?;
    conn.batch_execute(CREATE_DICTIONARY_SQL)?;
    doc.output("created `taxi_zone_dictionary`");

    doc.sql("SELECT dictGet('taxi_zone_dictionary', 'Borough', 132)");
    doc.rust("let borough: String = diesel::select(\n    sql::<Text>(\"dictGet('taxi_zone_dictionary', 'Borough', 132)\"),\n).get_result(&mut conn)?;");
    let borough: String = diesel::select(sql::<Text>(
        "dictGet('taxi_zone_dictionary', 'Borough', 132)",
    ))
    .get_result(&mut conn)?;
    doc.output(&borough);

    doc.sql("SELECT dictHas('taxi_zone_dictionary', 132)");
    doc.rust("let has_jfk: bool = diesel::select(\n    sql::<Bool>(\"dictHas('taxi_zone_dictionary', 132)\"),\n).get_result(&mut conn)?;");
    let has_jfk: bool = diesel::select(sql::<Bool>("dictHas('taxi_zone_dictionary', 132)"))
        .get_result(&mut conn)?;
    doc.output(&format!("dict has 132: {has_jfk}"));

    doc.sql("SELECT count(1) AS total, dictGetOrDefault('taxi_zone_dictionary','Borough', toUInt64(pickup_nyct2010_gid), 'Unknown') AS borough_name FROM trips WHERE dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138 GROUP BY borough_name ORDER BY total DESC");
    doc.rust("let borough_pickups: Vec<(i64, String)> = trips::table\n    .filter(dropoff_nyct2010_gid.eq(132_u8).or(dropoff_nyct2010_gid.eq(138_u8)))\n    .select((count_star(), pickup_borough_expr()))\n    .group_by(pickup_borough_expr())\n    .order(count_star().desc())\n    .load(&mut conn)?;");
    let borough_pickups: Vec<(i64, String)> = trips::table
        .filter(sql::<Bool>(
            "dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138",
        ))
        .group_by(pickup_borough_expr())
        .select((count_star(), pickup_borough_expr()))
        .order(count_star().desc())
        .load(&mut conn)?;
    doc.output(&format_rows(&borough_pickups));

    doc.heading(2, "Join with the dictionary");
    doc.sql("SELECT count(1) AS total, Borough AS borough FROM trips JOIN taxi_zone_dictionary ON toUInt64(trips.pickup_nyct2010_gid) = taxi_zone_dictionary.LocationID WHERE dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138 GROUP BY borough ORDER BY total DESC");
    doc.rust("#[derive(QueryableByName)]\nstruct BoroughTotal { /* total: i64, borough: String */ }\n\nlet joined: Vec<BoroughTotal> = diesel::sql_query(JOIN_SQL)\n    .load(&mut conn)?;");
    let joined: Vec<BoroughTotal> = diesel::sql_query(JOIN_SQL).load(&mut conn)?;
    let joined_rows = joined
        .iter()
        .map(|row| (row.total, row.borough.clone()))
        .collect::<Vec<_>>();
    doc.output(&format_rows(&joined_rows));

    let markdown = doc.finish();
    if let Some(path) = output {
        fs::write(&path, markdown)?;
        eprintln!("wrote {path}");
    } else {
        print!("{markdown}");
    }

    Ok(())
}

fn pickup_borough_expr() -> SqlLiteral<Text> {
    sql::<Text>(
        "dictGetOrDefault('taxi_zone_dictionary','Borough', toUInt64(`pickup_nyct2010_gid`), 'Unknown')",
    )
}

fn create_trips_sql() -> Result<String> {
    let ddl = create_table("trips")
        .if_not_exists()
        .column("trip_id", DataType::UInt32)
        .column("vendor_id", taxi_vendor_type())
        .column("pickup_date", DataType::Date)
        .column("pickup_datetime", DataType::DateTime)
        .column("dropoff_date", DataType::Date)
        .column("dropoff_datetime", DataType::DateTime)
        .column("store_and_fwd_flag", DataType::UInt8)
        .column("rate_code_id", DataType::UInt8)
        .column("pickup_longitude", DataType::Float64)
        .column("pickup_latitude", DataType::Float64)
        .column("dropoff_longitude", DataType::Float64)
        .column("dropoff_latitude", DataType::Float64)
        .column("passenger_count", DataType::UInt8)
        .column("trip_distance", DataType::Float64)
        .column("fare_amount", DataType::Float32)
        .column("extra", DataType::Float32)
        .column("mta_tax", DataType::Float32)
        .column("tip_amount", DataType::Float32)
        .column("tolls_amount", DataType::Float32)
        .column("ehail_fee", DataType::Float32)
        .column("improvement_surcharge", DataType::Float32)
        .column("total_amount", DataType::Float32)
        .column("payment_type", payment_type())
        .column("trip_type", DataType::UInt8)
        .column("pickup", DataType::custom("FixedString(25)"))
        .column("dropoff", DataType::custom("FixedString(25)"))
        .column("cab_type", cab_type())
        .column("pickup_nyct2010_gid", DataType::Int8)
        .column("pickup_ctlabel", DataType::Float32)
        .column("pickup_borocode", DataType::Int8)
        .column("pickup_ct2010", DataType::String)
        .column("pickup_boroct2010", DataType::String)
        .column("pickup_cdeligibil", DataType::String)
        .column("pickup_ntacode", DataType::custom("FixedString(4)"))
        .column("pickup_ntaname", DataType::String)
        .column("pickup_puma", DataType::UInt16)
        .column("dropoff_nyct2010_gid", DataType::UInt8)
        .column("dropoff_ctlabel", DataType::Float32)
        .column("dropoff_borocode", DataType::UInt8)
        .column("dropoff_ct2010", DataType::String)
        .column("dropoff_boroct2010", DataType::String)
        .column("dropoff_cdeligibil", DataType::String)
        .column("dropoff_ntacode", DataType::custom("FixedString(4)"))
        .column("dropoff_ntaname", DataType::String)
        .column("dropoff_puma", DataType::UInt16)
        .engine(
            merge_tree()
                .partition_by(["toYYYYMM(pickup_date)"])
                .order_by(["pickup_datetime"]),
        );
    Ok(to_sql(&ddl)?)
}

fn taxi_vendor_type() -> DataType {
    DataType::enum8([
        ("1", 1),
        ("2", 2),
        ("3", 3),
        ("4", 4),
        ("CMT", 5),
        ("VTS", 6),
        ("DDS", 7),
        ("B02512", 10),
        ("B02598", 11),
        ("B02617", 12),
        ("B02682", 13),
        ("B02764", 14),
        ("", 15),
    ])
}

fn payment_type() -> DataType {
    DataType::enum8([("UNK", 0), ("CSH", 1), ("CRE", 2), ("NOC", 3), ("DIS", 4)])
}

fn cab_type() -> DataType {
    DataType::enum8([("yellow", 1), ("green", 2), ("uber", 3)])
}

fn format_rows<T: std::fmt::Debug>(rows: &[T]) -> String {
    format!("{rows:#?}")
}

struct TutorialDoc {
    markdown: String,
}

impl TutorialDoc {
    fn new() -> Self {
        Self {
            markdown: String::new(),
        }
    }

    fn heading(&mut self, level: usize, title: &str) {
        self.markdown
            .push_str(&format!("\n{} {title}\n\n", "#".repeat(level)));
    }

    fn paragraph(&mut self, text: &str) {
        self.markdown.push_str(text);
        self.markdown.push_str("\n\n");
    }

    fn sql(&mut self, sql: &str) {
        self.code("sql", sql);
    }

    fn rust(&mut self, rust: &str) {
        self.code("rust", rust);
    }

    fn output(&mut self, output: &str) {
        self.code("text", output);
    }

    fn code(&mut self, language: &str, code: &str) {
        self.markdown.push_str("```");
        self.markdown.push_str(language);
        self.markdown.push('\n');
        self.markdown.push_str(code.trim());
        self.markdown.push_str("\n```\n\n");
    }

    fn finish(self) -> String {
        self.markdown.trim_start().to_owned()
    }
}

const CREATE_TRIPS_RUST: &str = r#"let ddl = create_table("trips")
    .if_not_exists()
    .column("trip_id", DataType::UInt32)
    .column("vendor_id", DataType::enum8([("1", 1), /* ... */ ("", 15)]))
    .column("pickup_date", DataType::Date)
    .column("pickup_datetime", DataType::DateTime)
    // ... remaining columns omitted here; see examples/tutorial.rs
    .engine(
        merge_tree()
            .partition_by(["toYYYYMM(pickup_date)"])
            .order_by(["pickup_datetime"]),
    );

diesel::sql_query(to_sql(&ddl)?).execute(&mut conn)?;"#;

const INSERT_TRIPS_SQL: &str = r#"
INSERT INTO trips
SELECT * FROM s3(
    'https://datasets-documentation.s3.eu-west-3.amazonaws.com/nyc-taxi/trips_{1..2}.gz',
    'TabSeparatedWithNames', "
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
") SETTINGS input_format_try_infer_datetimes = 0
"#;

const CREATE_DICTIONARY_SQL: &str = r#"
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
LAYOUT(HASHED_ARRAY())
"#;

const JOIN_SQL: &str = r#"
SELECT
    count(1) AS total,
    Borough AS borough
FROM trips
JOIN taxi_zone_dictionary
    ON toUInt64(trips.pickup_nyct2010_gid) = taxi_zone_dictionary.LocationID
WHERE dropoff_nyct2010_gid = 132 OR dropoff_nyct2010_gid = 138
GROUP BY borough
ORDER BY total DESC
"#;
