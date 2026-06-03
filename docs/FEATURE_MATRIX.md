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
| ✅ | Diesel SQL rendering helper | `diesel_clickhouse::to_sql(&events.select(id))?` | Still useful for inspection and external-client execution. |
| 🧪 | Execute rendered SQL via `clickhouse` crate | `client.query(&to_sql(&query)?).bind(...).fetch_all()` | Live Docker test validates this workflow. |
| 🚧 🧪 | Diesel `Connection` adapter | `query.load::<T>(&mut conn)?` | Initial HTTP-backed `ClickHouseConnection` supports:<br>`establish`, explicit `ClickHouseConnectionOptions`, `load`, `execute`, `batch_execute`<br>Server-side HTTP bind support with escaped-literal fallback where needed<br>Primitive/text/nullable rows, arrays, maps, and tuple decoding<br>`Array<T>`→`Vec<T>`<br>`Map<K,V>`→`BTreeMap<K,V>`<br>`Tuple<...>`<br>String-form `Decimal/Date/DateTime/UUID/IP/JSON/Dynamic/Variant` decoding<br>Optional `bigdecimal` support for `Numeric` and Decimal32/64/128/256<br>`sql_query`<br>Transactions are intentionally unsupported; this backend is blocking and owns a current-thread runtime<br>Use from blocking context (not directly from `async fn`). |
| ✅ 🧪 | Single-row insert | `insert_into(t).values((c.eq(v), ...)).execute(&mut conn)` | Tuple and `#[derive(Insertable)]` (with `#[diesel(treat_none_as_default_value = false)]`)<br>single-row inserts render and execute. |
| ⬜ | Multi-row batch insert via Diesel | `insert_into(t).values(vec![...])` | Not expressible: Diesel's `BatchInsert` requires the SQL `DEFAULT` keyword.<br>orphan-rule-reserved escape-hatch in a backend-specific `QueryFragment` impl for Diesel's own backends.<br>Use the `clickhouse` client's RowBinary `insert()`/`inserter()` for high-throughput batch ingestion. |

## ClickHouse SQL types

| Status | Feature | Example |
| --- | --- | --- |
| ✅ | Unsigned integers | `UInt8`, `UInt16`, `UInt32`, `UInt64` |
| ✅ | Signed ClickHouse-only integer | `Int8` |
| ✅ | `DateTime64` | `DateTime64` |
| ✅ | `BFloat16` | `BFloat16`, `DataType::BFloat16` |
| ✅ | `UUID` | `Uuid` |
| ✅ | `JSON` marker | `Json` |
| ✅ | `Array(T)` | `Array<Text>` |
| ✅ | `Map(K, V)` | `Map<Text, Text>` |
| ✅ | `LowCardinality(T)` | `LowCardinality<Text>` |
| ✅ | `Nothing` | `Nothing` |
| ✅ 🧪 | Aggregate state values | `AggregateFunction<T>`, DDL `DataType::aggregate_function("sum", [DataType::Float64])` |
| ✅ 🧪 | Vector embeddings | `Array<Float>`/`Array<Double>` columns, `vector_f32([..])`, `vector_f64([..])` |
| ✅ 🧪 | Nullable values | Diesel's `Nullable<T>`, `.is_null()`, `.is_not_null()` |
| ✅ 🧪 | Wider integers | `UInt128`, `UInt256`, `Int128`, `Int256`; DDL `DataType::{UInt128, UInt256, Int128, Int256}` |
| ✅ 🧪 | Decimal families | `Decimal32<S>`<br>`Decimal64<S>`<br>`Decimal128<S>`<br>`Decimal256<S>`<br>DDL `DataType::decimal64(4)`<br>Optional `bigdecimal` feature for native Rust loading/binds |
| ✅ 🧪 | Tuple / Nested / Enum | `Tuple<...>`<br>`Nested<...>`<br>`Enum8`, `Enum16`<br>DDL `DataType::tuple(...)`<br>`DataType::nested(...)`<br>`DataType::enum8(...)` |
| ✅ 🧪 | Network / Geo / semi-structured types | `IPv4`, `IPv6`, `Point`, `Ring`, `Dynamic`, `Variant`<br>DDL `DataType::{Point, Ring}`<br>`DataType::dynamic_with_max_types(4)`<br>`DataType::variant(...)` |

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
| ✅ 🧪 | `WHERE` | `events.filter(tenant_id.eq("acme").and(success.eq(true)))` | Diesel built-in, covered with ClickHouse rendering/live examples. |
| ✅ 🧪 | `HAVING` | `query.having(count_star().gt(1))` | Diesel built-in, covered with ClickHouse rendering/live examples. |
| ✅ 🧪 | `ORDER BY` | `query.order(created_at.desc())` | Diesel built-in, covered with ClickHouse rendering/live examples. |
| ✅ 🧪 | `LIMIT` / `OFFSET` | `query.limit(10).offset(20)` | Diesel built-in, covered with ClickHouse rendering/live examples. |
| ✅ 🧪 | `LIMIT ... WITH TIES` | `query.limit(1).with_ties()` | `LIMIT ? WITH TIES` |
| ✅ 🧪 | `LIMIT BY` | `query.limit_by_col(2, "tenant_id")` | `LIMIT 2 BY tenant_id` |
| ✅ 🧪 | `SETTINGS` | `query.settings([Setting::new("max_threads", 1)])` | `SETTINGS max_threads = 1` |
| ✅ | `FORMAT` | `query.format(Format::JsonEachRow)` | `FORMAT JSONEachRow` |
| ✅ 🧪 | Common table expressions | `.with_cte("name", subquery)` | `WITH name AS (SELECT ...)` |
| ✅ | Materialized CTEs | `.with_materialized_cte("name", subquery)` | `WITH name AS MATERIALIZED (...)` |
| ✅ 🧪 | `QUALIFY` | `query.qualify(row_number().over_ch(...).eq(1))` | `QUALIFY ...` |
| ✅ 🧪 | `WINDOW` clause | `query.window("w", partition_by(...).order_by(...))` | `WINDOW w AS (...)` |
| ✅ 🧪 | `ORDER BY ... WITH FILL` | `order(with_fill(ts).from(a).to(b).step(s))` | `ORDER BY ts WITH FILL ...` |
| ✅ | `INTO OUTFILE` | `query.into_outfile("export.csv").truncate().format(Format::Csv)` | Render-tested only; ClickHouse docs note it is CLI/local-client functionality and fails via HTTP. |

## GROUP BY extensions

| Status | Feature | Example DSL | SQL shape |
| --- | --- | --- | --- |
| ✅ 🧪 | Plain `GROUP BY` | `query.group_by(tenant_id)` | Diesel built-in, covered with ClickHouse rendering/live examples. |
| ✅ | `WITH TOTALS` | `query.group_by(with_totals(tenant_id))` | `GROUP BY tenant_id WITH TOTALS` |
| ✅ 🧪 | `ROLLUP` | `query.group_by(rollup((tenant_id, success)))` | `GROUP BY ROLLUP(tenant_id, success)` |
| ✅ | `CUBE` | `query.group_by(cube((tenant_id, success)))` | `GROUP BY CUBE(tenant_id, success)` |
| ✅ 🧪 | `GROUPING SETS` | `query.group_by(grouping_sets([vec!["tenant_id"], vec![]]))` | `GROUP BY GROUPING SETS (...)` |
| ✅ | `GROUPING()` function | `grouping((a, b))` | `GROUPING(a, b)` |
| ✅ 🧪 | `GROUP BY ALL` | `query.group_by(group_by_all())` | `GROUP BY ALL` |

## Joins

| Status | Feature | Example DSL | Notes |
| --- | --- | --- | --- |
| ✅ | Diesel ANSI join rendering | Diesel `.inner_join(...on(...))` | Render-tested. Diesel renders parenthesized join sources that ClickHouse rejects as a table expression.<br>Use `clickhouse_join(...)` for executable ClickHouse joins. |
| ✅ 🧪 | `GLOBAL JOIN` | `events.clickhouse_join(dim).global().any().inner().using(["tenant_id"])` | Custom ClickHouse join source; select columns with `join_column(...)`. |
| ✅ 🧪 | Typed join projection | `.select((join_column(events::id), join_column(tenants::plan)))` | `join_column` wraps a table column for selecting from `ClickHouseJoin` while preserving SQL type.<br>Replaces hand-written `sql::<...>(...)` lists and keeps typed reads.<br>Does not verify the column table is present in the join (orphan-rule limit). |
| ✅ 🧪 | Join strictness | `.any()`, `.all()`, `.asof()` | ClickHouse join grammar with optional `GLOBAL` and<br>strictness modifiers (`ANY`, `ALL`, `ASOF`), plus join kinds. |
| ✅ 🧪 | `SEMI` / `ANTI` joins | `.left().semi().using(...)`, `.left().anti().using(...)` | ClickHouse-specific join kinds. |
| ✅ 🧪 | `USING` / `ON` helpers | `.using(["tenant_id"])`, `.on(predicate)` | `ON`/`USING` use real, type-checked columns; wrap projected columns with `join_column(...)`. |

## Operators and predicates

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ | `GLOBAL IN` | `tenant_id.global_in(subquery)` |
| ✅ | `GLOBAL NOT IN` | `tenant_id.not_global_in(subquery)` |
| ✅ 🧪 | Regular comparison/logical operators | Diesel `.eq()`, `.gt()`, `.and()`, `.or()` built-ins. |
| ✅ 🧪 | ClickHouse lambda operators | `lambda("x", "x > 0")`, `lambda2("k", "v", "v != ''")` for higher-order array/map helpers. |
| ✅ 🧪 | `LIKE` variants / regexp helpers | Diesel `.like()` / `.not_like()`<br>ClickHouse `.ilike()` / `.not_ilike()`<br>`like`, `like_escape`, `ilike`, `not_ilike`<br>`regexp_match`, `multi_match_any`, `multi_match_any_index`, `multi_fuzzy_match_*` |

## Scalar functions

| Status | Feature group | Implemented examples | Missing examples |
| --- | --- | --- | --- |
| ✅ 🧪 | Date/time conversion and bucketing | `to_date`<br>`to_date_time`<br>`to_date_time64`<br>`to_start_of_*`<br>`date_diff`<br>`date_trunc`<br>`to_year`<br>`to_month`<br>`to_hour` | intervals, timezone variants, `now*`, `parseDateTime*` |
| ✅ 🧪 | Conditional/basic/numeric helpers | `if_`, `length`, `empty`, `not_empty`, `int_div`, `abs`, `round`, `floor`, `ceil`, `least`, `greatest` | `multiIf`, `coalesce`, `assumeNotNull` |
| ✅ 🧪 | Arrays | `has`<br>`has_any`<br>`has_all`<br>`array_join`<br>`array_element`<br>`array_concat`<br>`array_distinct`<br>`array_map`<br>`array_filter`<br>`array_exists`<br>`array_all`<br>`array_count` | More specialized helpers like `arrayFirst`, `arrayFold`, `arrayZip` can be added by demand. |
| ✅ 🧪 | Maps | `map_keys`, `map_values`, `map_contains`, `map_from_arrays`, `map_apply`, `map_filter` | Subscript and more specialized map helpers planned. |
| ✅ 🧪 | JSON | `json_extract_*`<br>`json_extract_*_path`<br>`json_extract_*_ci`<br>`json_value`<br>`json_query`<br>`json_exists`<br>`json_has`<br>`json_length`<br>`simple_json_extract_*`<br>`is_valid_json` | Dynamic JSON subcolumn helpers remain planned; case-insensitive helpers are render-tested because ClickHouse docs mark them v25.8+. |
| ✅ 🧪 | Strings | `lower`<br>`upper`<br>`substring`<br>`position`<br>`replace_all`<br>`concat`<br>`regexp_match`<br>`like`<br>`ilike`<br>`multi_match_any`<br>`multi_match_any_index`<br>`multi_match_all_indices`<br>`multi_fuzzy_match_*` | Token functions and specialized search variants can be added by demand. |
| ✅ 🧪 | URL/IP/encoding/hash | `domain`<br>`domain_without_www`<br>`top_level_domain`<br>`url_path`<br>`base64_encode`<br>`hex`<br>`city_hash64`<br>`to_ipv4`<br>`is_ipv6_string` | More specialized variants can be added by demand. |
| ✅ 🧪 | Vector distance/search | `l2_distance(embedding, vector_f32([..]))`<br>`cosine_distance`<br>`l1_distance`<br>`linf_distance`<br>`l2_norm` | Exact vector search via `ORDER BY distance ASC LIMIT n`; approximate index DDL below. |
| ✅ 🧪 | Type conversion | `to_int*`<br>`to_uint*`<br>`to_float*`<br>`to_*_or_null`<br>`to_*_or_zero`<br>`to_string`<br>`cast::<ST, _>(...)`<br>`accurate_cast*`<br>`is_null`<br>`is_not_null` | More date/decimal-specific conversion variants can be added by demand. |

## Vector search

ClickHouse vector search stores embeddings in array columns and orders by distance functions for exact search. Approximate search uses MergeTree `vector_similarity` skipping indexes. Reference docs: <https://clickhouse.com/docs/knowledgebase/vector-search> and <https://clickhouse.com/docs/engines/table-engines/mergetree-family/annindexes>.

| Status | Feature | Example DSL | Notes |
| --- | --- | --- | --- |
| ✅ 🧪 | Vector literals | `vector_f32([1.0, 0.0])`, `vector_f64([1.0, 2.0])` | Render as ClickHouse array literals. |
| ✅ 🧪 | Exact vector search | `query.order(l2_distance(embedding, vector_f32([...])).asc()).limit(10)` | Live test validates deterministic nearest-neighbor ordering. |
| ✅ | Approximate vector index DDL | `.index(vector_similarity_index("idx", "embedding", 1536)`<br>`.distance(VectorDistanceFunction::CosineDistance)` | Render-tested; live fixture keeps exact search portable across server builds. |
| ✅ 🧪 | `ALTER TABLE ... ADD/MATERIALIZE INDEX` | `alter_table("items").add_index(...)`, `.materialize_index("idx")` | Generic index lifecycle helpers work with vector indexes; live test uses a portable minmax index. |
| ✅ | Binary reference-vector parameter helpers | `vector_f32_binary(sql("$v"))`<br>`vector_f32_hex(sql("?"))`<br>`vector_f32_le_hex([...])` | Render-tested. ClickHouse docs recommend true binary client parameters;<br>the HTTP test client string-binds placeholders, so live coverage stays on exact vector literals. |

## Aggregate functions and combinators

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ 🧪 | Conditional aggregates | `count_if(success)`, `sum_if(x, pred)`, `avg_if(x, pred)`, `min_if(x, pred)`, `max_if(x, pred)` |
| ✅ 🧪 | Uniq aggregates | `uniq(x)`, `uniq_exact(x)`, `uniq_if(x, pred)`, `uniq_exact_if(x, pred)` |
| ✅ 🧪 | Array aggregates | `group_array(x)`, `group_array_if(x, pred)` |
| ✅ | Arg aggregates | `arg_max(arg, val)`, `arg_min(arg, val)` |
| ✅ 🧪 | Parametric quantiles | `quantile(0.95, x)`<br>`quantile_exact(0.5, x)`<br>`quantile_tdigest(0.99, x)`<br>`quantile_timing(0.95, x)`<br>`quantile_deterministic(0.5, x, seed)`<br>`quantiles([0.25, 0.75], x)`<br>`quantiles_timing([...], x)` |
| ✅ 🧪 | `topK` / histograms | `top_k(10, x)`, `histogram(20, x)` |
| ✅ | Any-value aggregates | `any_value(x)`, `any_last(x)` |
| ✅ 🧪 | Statistical aggregates | `corr(x, y)`<br>`covar_pop(x, y)`<br>`covar_samp(x, y)`<br>`covar_pop_stable(x, y)`<br>`covar_samp_stable(x, y)`<br>`stddev_pop(x)`<br>`stddev_samp(x)`<br>`var_pop(x)`<br>`var_samp(x)`<br>`analysis_of_variance(x, group)`<br>`mann_whitney_u_test(x, sample)`<br>`approx_top_sum(n, value, weight)` | Stable variants included for covariance, stddev, and variance. |
| ✅ 🧪 | General aggregate combinator builder | `aggregate::<Double>("avg").arg(x).or_null().if_(pred)`<br>`aggregate::<BigInt>("count").no_args().if_(pred)`<br>`.distinct()`<br>`.state()`<br>`.merge_state()`<br>`.combinator("ForEach")` | Provides a typed escape hatch for ClickHouse aggregate suffix combinators.<br>Preserves one-off helpers for common cases. |
| ✅ 🧪 | State/merge combinators | `sum_state(x)`, `sum_merge(state)`, `count_state()`, `uniq_exact_merge(state)`, `finalize_aggregation(state)` | Includes `AggregateFunction<T>` type marker and DDL type rendering. |
| ✅ 🧪 | Approx/statistical long tail | `stddev*`, `var*`, `analysis_of_variance`, `mann_whitney_u_test`, `approx_top_sum`, `approx_top_sum_with_reserved` | Additional specialized aggregate families can be added by demand. |

## Window functions

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ 🧪 | Ranking functions | `row_number().over_ch(...)`, `rank().over_window("w")`, `dense_rank().over_ch(...)` |
| ✅ 🧪 | Offset functions | `lag(expr).over_ch(...)`<br>`lead(expr).over_ch(...)`<br>`lag_in_frame(expr, offset, default).over_ch(...)`<br>`lead_in_frame(...)` |
| ✅ | Value window functions | `first_value(expr).over_ch(...)`, `last_value(expr).over_ch(...)` |
| ✅ 🧪 | Window `.over_ch(...)` / named `.over_window(...)` | `function.over_ch(partition_by(...).order_by(...))` |
| ✅ 🧪 | Named `WINDOW` clause | `query.window("w", partition_by(...).order_by(...))` |
| ✅ 🧪 | Frame variants beyond current row | `.rows_between_preceding_and_following(1, 1)`<br>`.range_between_preceding_and_current_row(1)`<br>`.rows_between(WindowFrameBound::CurrentRow, WindowFrameBound::following(2))` |

## DDL and table engines

| Status | Feature | Example DSL |
| --- | --- | --- |
| ✅ 🧪 | `CREATE TABLE` builder | `create_table("events").column(...).engine(...)` |
| ✅ 🧪 | MergeTree family engines | `merge_tree()`<br>`replacing_merge_tree()`<br>`summing_merge_tree()`<br>`aggregating_merge_tree()`<br>`collapsing_merge_tree("sign")`<br>`versioned_collapsing_merge_tree("sign", "version")` |
| ✅ 🧪 | Engine modifiers | `.partition_by(...)`, `.primary_key(...)`, `.order_by(...)`, `.sample_by(...)`, `.ttl(...)`, `.setting(...)` |
| ✅ 🧪 | Special engines | `TableEngine::memory()`, `TableEngine::null()`, `distributed(...).sharding_key(...)`, `buffer(...)` |
| ✅ 🧪 | Column codecs/default/materialized/alias | `Column::new(...).default_expr(...)`, `.materialized_expr(...)`, `.alias_expr(...)`, `.codec(...)` |
| ✅ 🧪 | Secondary indexes/projections | `vector_similarity_index("idx", "embedding", dims)`<br>`TableIndex::custom(...)`<br>`projection("by_tenant", "SELECT ...")`<br>`alter_table(...).add_projection(...)` |
| ✅ 🧪 | Materialized views | `create_materialized_view("events_mv").to("target").as_select(query)` |
| ✅ 🧪 | `ALTER TABLE` helpers | `alter_table("events").add_column(...)`<br>`.rename_column(...)`<br>`.add_index(...)`<br>`.materialize_index(...)`<br>`.add_projection(...)`<br>`.materialize_projection(...)`<br>`.update(...)`<br>`.delete_where(...)`<br>`.drop_partition(...)`<br>`.detach_partition(...)`<br>`.attach_partition(...)`<br>`.freeze_partition_with_name(...)` | Column/index/projection lifecycle and `MODIFY TTL`.<br>Mutations and common partition operations are implemented.<br>Advanced replicated-only partition moves/fetches can be added by demand. |

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
