# diesel-clickhouse feature matrix

This document tracks ClickHouse SQL surface area that `diesel-clickhouse` should make expressible from Diesel ASTs. It doubles as a checklist for implementation work and as source material for future crate documentation examples.

Legend:

- ✅ implemented and covered by SQL rendering tests
- 🧪 covered by live ClickHouse integration tests
- 🚧 partially implemented; more typed ergonomics or coverage needed
- ⬜ planned / not implemented
- ➡️ generally handled by Diesel itself, but should be documented with ClickHouse examples

## Backend and execution model

| Status | Feature | Example DSL | Notes |
| --- | --- | --- | --- |
| ✅ | ClickHouse backend marker and query builder | `to_sql(&query)?` | Backtick identifiers, `?` placeholders. |
| ✅ | Diesel SQL rendering helper | `diesel_clickhouse::to_sql(&events.select(id))?` | Render-only; no `Connection` yet. |
| 🧪 | Execute rendered SQL via `clickhouse` crate | `client.query(&to_sql(&query)?).bind(...).fetch_all()` | Live Docker test validates this workflow. |
| ⬜ | Diesel `Connection` adapter | `query.load::<T>(&mut conn)?` | Larger architecture decision; not started. |

## ClickHouse SQL types

| Status | Feature | Example |
| --- | --- | --- |
| ✅ | Unsigned integers | `UInt8`, `UInt16`, `UInt32`, `UInt64` |
| ✅ | Signed ClickHouse-only integer | `Int8` |
| ✅ | `DateTime64` | `DateTime64` |
| ✅ | `UUID` | `Uuid` |
| ✅ | `JSON` marker | `Json` |
| ✅ | `Array(T)` | `Array<Text>` |
| ✅ | `Map(K, V)` | `Map<Text, Text>` |
| ✅ | `LowCardinality(T)` | `LowCardinality<Text>` |
| ✅ | `Nothing` | `Nothing` |
| ➡️ | Nullable values | Diesel's `Nullable<T>` |
| ⬜ | Wider integers | `UInt128`, `UInt256`, `Int128`, `Int256` |
| ⬜ | Decimal families | `Decimal32`, `Decimal64`, `Decimal128`, `Decimal256` |
| ⬜ | Tuple / Nested / Enum | `Tuple<...>`, `Nested<...>`, `Enum8`, `Enum16` |
| ⬜ | Network / Geo / semi-structured types | `IPv4`, `IPv6`, `Point`, `Ring`, `Dynamic`, `Variant` |

## SELECT source modifiers and clauses

| Status | Feature | Example DSL | SQL shape |
| --- | --- | --- | --- |
| ✅ 🧪 | `FINAL` | `final_table(events)` | `FROM events FINAL` |
| ✅ 🧪 | `SAMPLE` | `sample(events, 0.1)` | `FROM events SAMPLE ?` |
| ✅ 🧪 | `SAMPLE ... OFFSET` | `sample_offset(events, 1.0, 0.0)` | `FROM events SAMPLE ? OFFSET ?` |
| ✅ 🧪 | `PREWHERE` | `prewhere(events, tenant_id.eq("acme"))` | `PREWHERE tenant_id = ?` |
| ✅ 🧪 | `ARRAY JOIN` | `events.array_join_as(tags, "tag")` | `ARRAY JOIN tags AS tag` |
| ✅ | `LEFT ARRAY JOIN` | `events.left_array_join_as(tags, "tag")` | `LEFT ARRAY JOIN tags AS tag` |
| ✅ 🧪 | Scalar `WITH` aliases | `diesel::select(sql("x")).with_alias(sql("1"), "x")` | `WITH 1 AS x SELECT x` |
| ➡️ | `WHERE` | `events.filter(tenant_id.eq("acme"))` | Diesel built-in. |
| ➡️ | `HAVING` | `query.having(count_star.gt(1))` | Diesel built-in; add ClickHouse examples. |
| ➡️ | `ORDER BY` | `query.order(created_at.desc())` | Diesel built-in. |
| ➡️ | `LIMIT` / `OFFSET` | `query.limit(10).offset(20)` | Diesel built-in. |
| ✅ 🧪 | `LIMIT ... WITH TIES` | `query.limit(1).with_ties()` | `LIMIT ? WITH TIES` |
| ✅ 🧪 | `LIMIT BY` | `query.limit_by_col(2, "tenant_id")` | `LIMIT 2 BY tenant_id` |
| ✅ 🧪 | `SETTINGS` | `query.settings([Setting::new("max_threads", 1)])` | `SETTINGS max_threads = 1` |
| ✅ | `FORMAT` | `query.format(Format::JsonEachRow)` | `FORMAT JSONEachRow` |
| ✅ 🧪 | Common table expressions | `.with_cte("name", subquery)` | `WITH name AS (SELECT ...)` |
| ✅ | Materialized CTEs | `.with_materialized_cte("name", subquery)` | `WITH name AS MATERIALIZED (...)` |
| ✅ 🧪 | `QUALIFY` | `query.qualify(row_number().over(...).eq(1))` | `QUALIFY ...` |
| ✅ 🧪 | `WINDOW` clause | `query.window("w", partition_by(...).order_by(...))` | `WINDOW w AS (...)` |
| ✅ 🧪 | `ORDER BY ... WITH FILL` | `order(with_fill(ts).from(a).to(b).step(s))` | `ORDER BY ts WITH FILL ...` |
| ⬜ | `INTO OUTFILE` | planned final query wrapper | `INTO OUTFILE ...` |

## GROUP BY extensions

| Status | Feature | Example DSL | SQL shape |
| --- | --- | --- | --- |
| ➡️ | Plain `GROUP BY` | `query.group_by(tenant_id)` | Diesel built-in. |
| ✅ | `WITH TOTALS` | `query.group_by(with_totals(tenant_id))` | `GROUP BY tenant_id WITH TOTALS` |
| ✅ 🧪 | `ROLLUP` | `query.group_by(rollup((tenant_id, success)))` | `GROUP BY ROLLUP(tenant_id, success)` |
| ✅ | `CUBE` | `query.group_by(cube((tenant_id, success)))` | `GROUP BY CUBE(tenant_id, success)` |
| ✅ 🧪 | `GROUPING SETS` | `query.group_by(grouping_sets([vec!["tenant_id"], vec![]]))` | `GROUP BY GROUPING SETS (...)` |
| ✅ | `GROUPING()` function | `grouping((a, b))` | `GROUPING(a, b)` |
| ✅ 🧪 | `GROUP BY ALL` | `query.group_by(group_by_all())` | `GROUP BY ALL` |

## Joins

| Status | Feature | Example DSL | Notes |
| --- | --- | --- | --- |
| ➡️ | ANSI joins | Diesel `.inner_join`, `.left_join` | Need ClickHouse examples and live tests. |
| ⬜ | `GLOBAL JOIN` | planned join source wrapper | Similar to `GLOBAL IN`, but for joins. |
| ⬜ | Join strictness | planned `any_inner_join`, `all_left_join`, `asof_join` | ClickHouse `[GLOBAL] [ANY|ALL|ASOF] [INNER|LEFT|...] JOIN`. |
| ⬜ | `SEMI` / `ANTI` joins | planned join source wrappers | ClickHouse-specific join kinds. |
| ⬜ | `USING` helper | planned typed/raw column-list helper | Diesel mostly models `ON`; ClickHouse users often use `USING`. |

## Operators and predicates

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ | `GLOBAL IN` | `tenant_id.global_in(subquery)` |
| ✅ | `GLOBAL NOT IN` | `tenant_id.not_global_in(subquery)` |
| ➡️ | Regular comparison/logical operators | Diesel built-ins. |
| ⬜ | ClickHouse lambda operators | planned higher-order array helpers. |
| ⬜ | `LIKE` variants / regexp helpers | planned `match`, `multiMatch*`, `like` examples. |

## Scalar functions

| Status | Feature group | Implemented examples | Missing examples |
| --- | --- | --- | --- |
| ✅ 🧪 | Date/time conversion and bucketing | `to_date`, `to_date_time`, `to_date_time64`, `to_start_of_*`, `date_diff`, `date_trunc`, `to_year`, `to_month`, `to_hour` | intervals, timezone variants, `now*`, `parseDateTime*` |
| ✅ 🧪 | Conditional/basic/numeric helpers | `if_`, `length`, `empty`, `not_empty`, `int_div`, `abs`, `round`, `floor`, `ceil`, `least`, `greatest` | `multiIf`, `coalesce`, `assumeNotNull` |
| ✅ 🧪 | Arrays | `has`, `has_any`, `has_all`, `array_join`, `array_element`, `array_concat`, `array_distinct` | `arrayMap`, `arrayFilter`, `arrayExists`, lambda support |
| ✅ 🧪 | Maps | `map_keys`, `map_values`, `map_contains` | `mapApply`, `mapFilter`, `mapFromArrays`, subscript helpers |
| ✅ 🧪 | JSON | `json_extract_string`, `json_extract_int`, `json_extract_float`, `json_extract_bool`, `json_extract_raw` | case-insensitive variants, paths, dynamic JSON subcolumns |
| ✅ 🧪 | Strings | `lower`, `upper`, `substring`, `position`, `replace_all`, `concat`, `regexp_match` | more regexp/search variants, token functions |
| ⬜ | URL/IP/encoding/hash | planned | common analytics functions. |
| ✅ 🧪 | Type conversion | `to_int64`, `to_uint64`, `to_float64`, `to_string` | complete `toUInt*`/`toInt*` families, `CAST`, `accurateCast*` |

## Aggregate functions and combinators

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ 🧪 | Conditional aggregates | `count_if(success)`, `sum_if(x, pred)`, `avg_if(x, pred)`, `min_if(x, pred)`, `max_if(x, pred)` |
| ✅ 🧪 | Uniq aggregates | `uniq(x)`, `uniq_exact(x)`, `uniq_if(x, pred)`, `uniq_exact_if(x, pred)` |
| ✅ 🧪 | Array aggregates | `group_array(x)`, `group_array_if(x, pred)` |
| ✅ | Arg aggregates | `arg_max(arg, val)`, `arg_min(arg, val)` |
| ✅ 🧪 | Parametric quantiles | `quantile(0.95, x)`, `quantile_exact(0.5, x)`, `quantile_tdigest(0.99, x)`, `quantiles([0.25, 0.75], x)` |
| ✅ 🧪 | `topK` | `top_k(10, x)` |
| ✅ | Any-value aggregates | `any_value(x)`, `any_last(x)` |
| ⬜ | General aggregate combinator builder | planned `sum().if_(pred).or_null()`-style API | Could reduce one-off functions. |
| ⬜ | State/merge combinators | `sumState`, `sumMerge`, etc. | Needed for materialized views/aggregating engines. |
| ⬜ | More approximate/statistical aggregates | `quantileTiming`, `quantileDeterministic`, `corr`, `covar*`, `histogram` | Add by demand. |

## Window functions

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ 🧪 | Ranking functions | `row_number().over(...)`, `rank().over_window("w")`, `dense_rank().over(...)` |
| ✅ 🧪 | Offset functions | `lag(expr).over(...)`, `lead(expr).over(...)`, `lag_in_frame(expr, offset, default).over(...)`, `lead_in_frame(...)` |
| ✅ | Value window functions | `first_value(expr).over(...)`, `last_value(expr).over(...)` |
| ✅ 🧪 | Window `.over(...)` / named `.over_window(...)` | `function.over(partition_by(...).order_by(...))` |
| ✅ 🧪 | Named `WINDOW` clause | `query.window("w", partition_by(...).order_by(...))` |
| ⬜ | Frame variants beyond current row | planned rows/range preceding/following helpers |

## DDL and table engines

| Status | Feature | Example DSL |
| --- | --- | --- |
| ⬜ | `CREATE TABLE` builder | planned `create_table("events").column(...).engine(...)` |
| ⬜ | MergeTree family engines | planned `MergeTree::new().order_by(...).partition_by(...)` |
| ⬜ | Engine modifiers | planned `ttl`, `settings`, `sample_by`, `primary_key` |
| ⬜ | Column codecs/default/materialized/alias | planned column-definition builders |
| ⬜ | Secondary indexes/projections | planned DDL fragments |
| ⬜ | Materialized views | planned create-view builders |
| ⬜ | `ALTER TABLE` helpers | planned mutations, partitions, TTL/materialize index helpers |

## Test policy

Every implemented feature should have:

1. A SQL rendering assertion in `tests/sql_render.rs`.
2. A live ClickHouse assertion in `tests/live_clickhouse.rs` when ClickHouse can execute it deterministically in the Docker fixture.
3. A README or docs example once the API shape is stable.

Run the full current suite with:

```bash
cargo test
cargo test --test live_clickhouse -- --ignored --nocapture
cargo clippy --all-targets -- -D warnings
```
