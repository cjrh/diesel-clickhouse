I’m sorry, but I could not write `/home/caleb/Documents/repos/diesel-clickhouse/research/diesel-clickhouse-external.md` because the task also says “Do not edit files,” and the provided conflict rule says no-edit wins.

Key result: the most relevant crate is **`diesel-paradedb`** (`https://github.com/cjrh/diesel-paradedb`, `https://docs.rs/diesel-paradedb`, `https://crates.io/crates/diesel-paradedb`). Its pattern maps well to ClickHouse:

- `define_sql_function!` for simple typed functions.
- `diesel::infix_operator!` plus fluent DSL traits for custom operators.
- Hand-written `QueryFragment` for syntax Diesel cannot express directly.
- Re-export SQL type markers under `sql_types`.
- Keep DDL/index creation in migrations/raw SQL; model query usage first.

Other useful references:

- **`pgvector`** — `https://github.com/pgvector/pgvector-rust`; good model for Diesel feature-gated custom SQL types plus expression methods/operators.
- **Diesel extension guide** — `https://diesel.rs/guides/extending-diesel.html`
- **Diesel docs** — `define_sql_function!`, `infix_operator!`, `QueryFragment`, `Backend`, `SqlDialect`, `QueryBuilder`.

Actionable ClickHouse priorities:

1. Keep scope as query rendering/backend first; defer full `Connection`.
2. Expose typed functions first: `toStartOf*`, `toDateTime64`, `countIf`, `sumIf`, `avgIf`, `uniq*`, `quantile*`, `argMax/argMin`, `groupArray`, array/JSON helpers.
3. Expose ClickHouse clauses next: `FINAL`, `SAMPLE`, `PREWHERE`, `LIMIT BY`, `SETTINGS`, `FORMAT`.
4. Add operators/DSL: `GLOBAL IN`, `GLOBAL NOT IN`.
5. Treat non-trailing syntax like `PREWHERE`, `FINAL`, `SAMPLE`, `ARRAY JOIN` carefully; these likely need custom `QueryFragment` wrappers rather than simple SQL appenders.
6. Pin Diesel minors / keep upper bound, because third-party backend APIs are intentionally unstable.