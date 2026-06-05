//! Native async Diesel connection backed by ClickHouse's HTTP interface.
//!
//! The connection intentionally models ClickHouse as ClickHouse: transactions
//! are reported as unsupported, statement execution reports the row count
//! ClickHouse returns in its `X-ClickHouse-Summary` response trailer (when one
//! is available), and result loading uses ClickHouse's
//! `TabSeparatedWithNamesAndTypes` format as a simple transport for Diesel's row
//! deserializer.
//!
//! It implements [`diesel_async::AsyncConnection`] directly on top of the async
//! `clickhouse` client's futures — there is no hidden runtime and no `block_on`.
//! Drive it from an async context with `diesel_async::RunQueryDsl` and `.await`.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use diesel::connection::{
    CacheSize, Instrumentation, InstrumentationEvent, StrQueryHelper, TransactionManagerStatus,
    get_default_instrumentation,
};
use diesel::query_builder::{
    AsQuery, QueryBuilder, QueryFragment, QueryId, bind_collector::RawBytesBindCollector,
};
use diesel::result::{ConnectionError, ConnectionResult, DatabaseErrorKind, Error, QueryResult};
use diesel::row::{Field, PartialRow, Row, RowIndex, RowSealed};
use diesel_async::{
    AsyncConnection, AsyncConnectionCore, SimpleAsyncConnection, TransactionManager,
};
use futures_util::future::BoxFuture;
use futures_util::stream::{self, BoxStream};
use futures_util::{FutureExt, StreamExt};

use crate::backend::{ClickHouse, ClickHouseQueryBuilder, ClickHouseTypeMetadata};

/// A native async Diesel connection for ClickHouse over HTTP.
///
/// Implements [`diesel_async::AsyncConnection`], so it composes with
/// `diesel_async::RunQueryDsl` and the `bb8`/`deadpool`/`mobc` connection pools.
/// It supports idiomatic Diesel loading for primitive, text, nullable, and
/// common composite result values while keeping ClickHouse-specific semantics
/// explicit: transactions are unsupported, and command execution reports the
/// number of written rows ClickHouse declares in its `X-ClickHouse-Summary`
/// response trailer, falling back to `0` for statements (such as DDL) that
/// ClickHouse does not count.
///
/// # Async usage
///
/// Queries return futures; drive them from any async task with `.await`. The
/// connection drives the async `clickhouse` client directly, so — unlike a
/// blocking Diesel connection — there is no owned runtime and no restriction on
/// calling it from inside an `async fn`:
///
/// ```ignore
/// use diesel_async::{AsyncConnection, RunQueryDsl};
/// use diesel_clickhouse::AsyncClickHouseConnection;
///
/// let mut conn = AsyncClickHouseConnection::establish(database_url).await?;
/// let ids: Vec<i64> = events::table.select(events::id).load(&mut conn).await?;
/// ```
///
/// Callers stuck in a blocking context (or running `diesel_migrations`) can wrap
/// it with diesel-async's `AsyncConnectionWrapper` (enable the
/// `async-connection-wrapper` feature).
#[allow(missing_debug_implementations)]
pub struct AsyncClickHouseConnection {
    client: clickhouse::Client,
    transaction_state: TransactionManagerStatus,
    instrumentation: Option<Box<dyn Instrumentation>>,
}

/// Explicit configuration for establishing an [`AsyncClickHouseConnection`].
///
/// Use [`ClickHouseConnectionOptions::from_url`] for Diesel-style database URLs
/// with credentials, database, and query options encoded in one string. Use
/// [`ClickHouseConnectionOptions::new`] when those pieces should be assembled
/// explicitly in code. Options are passed directly to ClickHouse as HTTP query
/// settings; no semantic settings are injected by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickHouseConnectionOptions {
    url: String,
    user: Option<String>,
    password: Option<String>,
    database: Option<String>,
    options: BTreeMap<String, String>,
}

impl ClickHouseConnectionOptions {
    /// Create options from a ClickHouse HTTP endpoint such as
    /// `http://localhost:8123`.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            user: None,
            password: None,
            database: None,
            options: BTreeMap::new(),
        }
    }

    /// Parse the same URL shape accepted by [`AsyncClickHouseConnection::establish`].
    pub fn from_url(database_url: &str) -> ConnectionResult<Self> {
        let parsed = url::Url::parse(database_url)
            .map_err(|err| ConnectionError::InvalidConnectionUrl(err.to_string()))?;
        let mut options = Self::new(base_url(&parsed)?);

        if !parsed.username().is_empty() {
            options.user = Some(parsed.username().to_owned());
        }
        if let Some(password) = parsed.password() {
            options.password = Some(password.to_owned());
        }

        let mut database_from_query = None;
        for (name, value) in parsed.query_pairs() {
            match name.as_ref() {
                "user" if options.user.is_none() => options.user = Some(value.into_owned()),
                "password" if options.password.is_none() => {
                    options.password = Some(value.into_owned());
                }
                "database" => database_from_query = Some(value.into_owned()),
                option => {
                    options
                        .options
                        .insert(option.to_owned(), value.into_owned());
                }
            }
        }

        options.database = database_from_query.or_else(|| database_from_path(&parsed));
        Ok(options)
    }

    /// Set the ClickHouse user.
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set the ClickHouse password.
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the default ClickHouse database.
    pub fn database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Add or replace a ClickHouse HTTP query option/setting.
    pub fn option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(name.into(), value.into());
        self
    }

    /// Establish a connection using these explicit options.
    ///
    /// Builds the ClickHouse client and runs a `SELECT 1` health check so the
    /// returned connection is known to be reachable, mirroring
    /// [`AsyncClickHouseConnection::establish`].
    pub async fn connect(self) -> ConnectionResult<AsyncClickHouseConnection> {
        let mut client = clickhouse::Client::default()
            .with_url(&self.url)
            .with_product_info("diesel-clickhouse", env!("CARGO_PKG_VERSION"));

        if let Some(user) = self.user {
            client = client.with_user(user);
        }
        if let Some(password) = self.password {
            client = client.with_password(password);
        }
        if let Some(database) = self.database {
            client = client.with_database(database);
        }
        for (name, value) in self.options {
            client = client.with_setting(name, value);
        }

        let mut conn = AsyncClickHouseConnection::with_client(client);
        conn.execute_sql("SELECT 1 FORMAT Null")
            .await
            .map_err(ConnectionError::CouldntSetupConfiguration)?;
        Ok(conn)
    }
}

impl AsyncClickHouseConnection {
    /// Build a connection around an already-configured ClickHouse client.
    ///
    /// This is a pure constructor: it performs no I/O and does not health-check
    /// the endpoint. Use [`AsyncClickHouseConnection::establish`] or
    /// [`ClickHouseConnectionOptions::connect`] when you want the `SELECT 1`
    /// reachability check.
    pub fn with_client(client: clickhouse::Client) -> Self {
        Self {
            client,
            transaction_state: TransactionManagerStatus::default(),
            instrumentation: get_default_instrumentation(),
        }
    }

    /// Access the underlying ClickHouse client for ClickHouse-specific setup.
    pub fn client(&self) -> &clickhouse::Client {
        &self.client
    }

    /// Insert many rows in a single columnar RowBinary request, returning the
    /// number of rows sent.
    ///
    /// Diesel's multi-row `INSERT` cannot be expressed on a third-party backend
    /// — its `BatchInsert` requires the SQL `DEFAULT` keyword that the orphan
    /// rule reserves for Diesel's own backends — and looping single-row Diesel
    /// inserts pays one escaped-text round trip per row. This drives the
    /// `clickhouse` client's native RowBinary inserter instead: the whole batch
    /// is encoded columnar and sent as one request, which is the high-throughput
    /// write path ClickHouse is built for.
    ///
    /// `Row` is a `#[derive(clickhouse::Row, serde::Serialize)]` struct whose
    /// fields name and type the target columns; `table` is the unquoted table
    /// name. The batch is validated against the table's schema before sending
    /// (a one-time metadata fetch). The whole insert is atomic from the client's
    /// perspective: it is only committed when the final request returns `200`,
    /// so an error means no rows from this call were accepted.
    ///
    /// ```ignore
    /// #[derive(clickhouse::Row, serde::Serialize)]
    /// struct EventRow {
    ///     id: u64,
    ///     tenant_id: String,
    /// }
    ///
    /// let written = conn.insert_batch("events", events).await?;
    /// ```
    ///
    /// For long-running, periodically-flushed ingestion, reach for the client's
    /// `inserter(...)` directly via [`client`](Self::client).
    pub async fn insert_batch<Row, Rows>(&mut self, table: &str, rows: Rows) -> QueryResult<usize>
    where
        Row: clickhouse::RowOwned + clickhouse::RowWrite,
        Rows: IntoIterator<Item = Row>,
    {
        let mut insert = self
            .client
            .insert::<Row>(table)
            .await
            .map_err(clickhouse_error)?;
        let mut written = 0usize;
        for row in rows {
            insert.write(&row).await.map_err(clickhouse_error)?;
            written += 1;
        }
        insert.end().await.map_err(clickhouse_error)?;
        Ok(written)
    }

    fn render_query<T>(&mut self, source: &T) -> QueryResult<PreparedQuery>
    where
        T: QueryFragment<ClickHouse> + QueryId,
    {
        let backend = ClickHouse;
        let mut query_builder = ClickHouseQueryBuilder::default();
        source.to_sql(&mut query_builder, &backend)?;
        let sql = query_builder.finish();

        // ClickHouse's `TypeMetadata::MetadataLookup` is `()`, so the bind
        // collector needs no connection-held lookup state.
        let mut metadata_lookup = ();
        let mut bind_collector = RawBytesBindCollector::<ClickHouse>::new();
        source.collect_binds(&mut bind_collector, &mut metadata_lookup, &backend)?;
        parameterize_binds(&sql, &bind_collector.metadata, &bind_collector.binds)
    }

    async fn execute_sql(&mut self, sql: &str) -> QueryResult<()> {
        self.execute_prepared(PreparedQuery::plain(sql)).await?;
        Ok(())
    }

    /// Run a statement that yields no Diesel result set, returning the number of
    /// rows ClickHouse reports writing when it provides one.
    ///
    /// This deliberately uses `fetch_bytes` rather than the `clickhouse`
    /// client's fire-and-forget `execute`: `execute` discards the response,
    /// while the byte cursor exposes the parsed `X-ClickHouse-Summary` trailer
    /// that carries `written_rows`. `wait_end_of_query=1` makes ClickHouse
    /// finish the statement before responding, so the summary reflects the
    /// completed write instead of mid-flight progress. The response body is
    /// empty for the DDL/DML this path runs (and `FORMAT Null` health checks
    /// override the requested format), so collecting and dropping it is cheap.
    async fn execute_prepared(&mut self, prepared: PreparedQuery) -> QueryResult<Option<u64>> {
        let query = StrQueryHelper::new(&prepared.sql);
        self.instrumentation
            .on_connection_event(InstrumentationEvent::start_query(&query));

        let client_sql = escape_clickhouse_client_template(&prepared.sql);
        let result = async {
            let mut http_query = self
                .client
                .query(&client_sql)
                .with_setting("wait_end_of_query", "1");
            for (name, value) in prepared.params {
                http_query = http_query.with_setting(name, value);
            }
            let mut cursor = http_query
                .fetch_bytes("TabSeparatedWithNamesAndTypes")
                .map_err(clickhouse_error)?;
            cursor.collect().await.map_err(clickhouse_error)?;
            Ok::<_, Error>(cursor.summary().and_then(|summary| summary.written_rows()))
        }
        .await;

        self.instrumentation
            .on_connection_event(InstrumentationEvent::finish_query(
                &query,
                result.as_ref().err(),
            ));
        result
    }

    async fn load_prepared(&mut self, prepared: PreparedQuery) -> QueryResult<Vec<ClickHouseRow>> {
        let query = StrQueryHelper::new(&prepared.sql);
        self.instrumentation
            .on_connection_event(InstrumentationEvent::start_query(&query));

        let client_sql = escape_clickhouse_client_template(&prepared.sql);
        let result = async {
            let mut http_query = self.client.query(&client_sql);
            for (name, value) in prepared.params {
                http_query = http_query.with_setting(name, value);
            }
            let mut cursor = http_query
                .fetch_bytes("TabSeparatedWithNamesAndTypes")
                .map_err(clickhouse_error)?;
            cursor.collect().await.map_err(clickhouse_error)
        }
        .await;

        let result = result.and_then(|bytes| parse_rows(&bytes));
        self.instrumentation
            .on_connection_event(InstrumentationEvent::finish_query(
                &query,
                result.as_ref().err(),
            ));
        result
    }
}

impl SimpleAsyncConnection for AsyncClickHouseConnection {
    async fn batch_execute(&mut self, query: &str) -> QueryResult<()> {
        for statement in split_statements(query) {
            self.execute_sql(statement).await?;
        }
        Ok(())
    }
}

impl AsyncConnectionCore for AsyncClickHouseConnection {
    type ExecuteFuture<'conn, 'query> = BoxFuture<'conn, QueryResult<usize>>;
    type LoadFuture<'conn, 'query> = BoxFuture<'conn, QueryResult<Self::Stream<'conn, 'query>>>;
    type Stream<'conn, 'query> = BoxStream<'conn, QueryResult<ClickHouseRow>>;
    type Row<'conn, 'query> = ClickHouseRow;
    type Backend = ClickHouse;

    fn load<'conn, 'query, T>(&'conn mut self, source: T) -> Self::LoadFuture<'conn, 'query>
    where
        T: AsQuery + 'query,
        T::Query: QueryFragment<Self::Backend> + QueryId + 'query,
    {
        // Render to SQL + binds synchronously so the returned future owns only
        // `Send` data and never captures the (possibly non-`Send`) query `T`.
        let query = source.as_query();
        let prepared = self.render_query(&query);
        async move {
            let rows = self.load_prepared(prepared?).await?;
            Ok(stream::iter(rows.into_iter().map(Ok)).boxed())
        }
        .boxed()
    }

    fn execute_returning_count<'conn, 'query, T>(
        &'conn mut self,
        source: T,
    ) -> Self::ExecuteFuture<'conn, 'query>
    where
        T: QueryFragment<Self::Backend> + QueryId + 'query,
    {
        let prepared = self.render_query(&source);
        async move {
            // ClickHouse reports written rows in its `X-ClickHouse-Summary`
            // trailer; statements it does not count — DDL, and the background
            // mutations behind some `ALTER ... DELETE`/`UPDATE` forms — omit it,
            // so report `0` rather than guess. See `docs/CONNECTION_DESIGN.md`.
            let written_rows = self.execute_prepared(prepared?).await?;
            Ok(written_rows.unwrap_or(0) as usize)
        }
        .boxed()
    }
}

impl AsyncConnection for AsyncClickHouseConnection {
    type TransactionManager = ClickHouseTransactionManager;

    async fn establish(database_url: &str) -> ConnectionResult<Self> {
        let mut instrumentation = get_default_instrumentation();
        instrumentation.on_connection_event(InstrumentationEvent::start_establish_connection(
            database_url,
        ));

        let result = match ClickHouseConnectionOptions::from_url(database_url) {
            Ok(options) => options.connect().await,
            Err(err) => Err(err),
        };

        instrumentation.on_connection_event(InstrumentationEvent::finish_establish_connection(
            database_url,
            result.as_ref().err(),
        ));

        let mut conn = result?;
        conn.instrumentation = instrumentation;
        Ok(conn)
    }

    fn transaction_state(&mut self) -> &mut TransactionManagerStatus {
        &mut self.transaction_state
    }

    fn instrumentation(&mut self) -> &mut dyn Instrumentation {
        &mut self.instrumentation
    }

    fn set_instrumentation(&mut self, instrumentation: impl Instrumentation) {
        self.instrumentation = Some(Box::new(instrumentation));
    }

    fn set_prepared_statement_cache_size(&mut self, _size: CacheSize) {
        // Each query goes over HTTP without a prepared-statement cache, so
        // Diesel's cache-size knob is intentionally a no-op.
    }
}

/// Transaction manager that makes unsupported ClickHouse transactions explicit.
///
/// ClickHouse has no general multi-statement transactions, so every begin /
/// commit / rollback resolves to the same "unsupported" error rather than
/// silently pretending to start a transaction.
#[derive(Debug, Default)]
pub struct ClickHouseTransactionManager;

impl TransactionManager<AsyncClickHouseConnection> for ClickHouseTransactionManager {
    type TransactionStateData = TransactionManagerStatus;

    async fn begin_transaction(_conn: &mut AsyncClickHouseConnection) -> QueryResult<()> {
        Err(unsupported_transactions())
    }

    async fn rollback_transaction(_conn: &mut AsyncClickHouseConnection) -> QueryResult<()> {
        Err(unsupported_transactions())
    }

    async fn commit_transaction(_conn: &mut AsyncClickHouseConnection) -> QueryResult<()> {
        Err(unsupported_transactions())
    }

    fn transaction_manager_status_mut(
        conn: &mut AsyncClickHouseConnection,
    ) -> &mut TransactionManagerStatus {
        &mut conn.transaction_state
    }
}

/// Lets the `bb8`, `deadpool`, and `mobc` pools manage this connection.
///
/// The default `PoolableConnection` behaviour fits ClickHouse: `ping` issues a
/// `SELECT 1`, and `is_broken` consults the (always-open) transaction manager.
#[cfg(any(feature = "bb8", feature = "deadpool", feature = "mobc"))]
impl diesel_async::pooled_connection::PoolableConnection for AsyncClickHouseConnection {}

/// Column names and name→index lookup shared by every row in one result set.
///
/// A ClickHouse result repeats the same column names for every row, so the
/// header is computed once during parsing and shared (`Arc`) across all rows
/// instead of being cloned per row. This keeps loading a large result set from
/// allocating one `HashMap` and N column-name `String`s per row.
#[derive(Debug)]
struct RowHeader {
    names: Vec<String>,
    by_name: HashMap<String, usize>,
}

/// Owned result row used by the ClickHouse connection.
///
/// Holds only this row's field values; column names live in a shared
/// `RowHeader`.
#[derive(Debug, Clone)]
pub struct ClickHouseRow {
    header: Arc<RowHeader>,
    values: Vec<Option<Vec<u8>>>,
}

/// Field view returned from [`ClickHouseRow`].
#[derive(Debug, Clone, Copy)]
pub struct ClickHouseField<'a> {
    name: &'a str,
    value: Option<&'a [u8]>,
}

impl RowSealed for ClickHouseRow {}

impl RowIndex<usize> for ClickHouseRow {
    fn idx(&self, idx: usize) -> Option<usize> {
        (idx < self.values.len()).then_some(idx)
    }
}

impl RowIndex<&str> for ClickHouseRow {
    fn idx(&self, idx: &str) -> Option<usize> {
        self.header.by_name.get(idx).copied()
    }
}

impl<'a> Row<'a, ClickHouse> for ClickHouseRow {
    type Field<'f>
        = ClickHouseField<'f>
    where
        'a: 'f,
        Self: 'f;

    type InnerPartialRow = Self;

    fn field_count(&self) -> usize {
        self.values.len()
    }

    fn get<'b, I>(&'b self, idx: I) -> Option<Self::Field<'b>>
    where
        'a: 'b,
        Self: RowIndex<I>,
    {
        let idx = self.idx(idx)?;
        let name = self.header.names.get(idx)?;
        let value = self.values.get(idx)?;
        Some(ClickHouseField {
            name,
            value: value.as_deref(),
        })
    }

    fn partial_row(&self, range: Range<usize>) -> PartialRow<'_, Self::InnerPartialRow> {
        PartialRow::new(self, range)
    }
}

impl<'a> Field<'a, ClickHouse> for ClickHouseField<'a> {
    fn field_name(&self) -> Option<&str> {
        Some(self.name)
    }

    fn value(&self) -> Option<<ClickHouse as diesel::backend::Backend>::RawValue<'_>> {
        self.value
    }
}

fn base_url(parsed: &url::Url) -> ConnectionResult<String> {
    let mut base = parsed.clone();
    base.set_username("").map_err(|_| {
        ConnectionError::InvalidConnectionUrl("could not strip username from ClickHouse URL".into())
    })?;
    base.set_password(None).map_err(|_| {
        ConnectionError::InvalidConnectionUrl("could not strip password from ClickHouse URL".into())
    })?;
    base.set_path("");
    base.set_query(None);
    base.set_fragment(None);
    Ok(base.to_string())
}

fn database_from_path(parsed: &url::Url) -> Option<String> {
    parsed
        .path_segments()?
        .find(|segment| !segment.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone)]
struct PreparedQuery {
    sql: String,
    params: Vec<(String, String)>,
}

impl PreparedQuery {
    fn plain(sql: &str) -> Self {
        Self {
            sql: sql.to_owned(),
            params: Vec::new(),
        }
    }
}

fn parameterize_binds(
    sql: &str,
    metadata: &[ClickHouseTypeMetadata],
    binds: &[Option<Vec<u8>>],
) -> QueryResult<PreparedQuery> {
    let mut result = String::with_capacity(sql.len());
    let mut params = Vec::new();
    let mut chars = sql.char_indices().peekable();
    let mut bind_idx = 0;
    let mut state = SqlScanState::Code;

    while let Some((_, ch)) = chars.next() {
        match state {
            SqlScanState::Code => match ch {
                '?' => {
                    if matches!(chars.peek(), Some((_, '?'))) {
                        chars.next();
                        result.push('?');
                    } else {
                        push_bind_parameter_or_literal(
                            &mut result,
                            &mut params,
                            bind_idx,
                            metadata,
                            binds,
                        )?;
                        bind_idx += 1;
                    }
                }
                '\'' => {
                    result.push(ch);
                    state = SqlScanState::SingleQuoted { escaped: false };
                }
                '"' => {
                    result.push(ch);
                    state = SqlScanState::DoubleQuoted { escaped: false };
                }
                '`' => {
                    result.push(ch);
                    state = SqlScanState::BacktickQuoted;
                }
                '-' if matches!(chars.peek(), Some((_, '-'))) => {
                    chars.next();
                    result.push_str("--");
                    state = SqlScanState::LineComment;
                }
                '#' => {
                    result.push(ch);
                    state = SqlScanState::LineComment;
                }
                '/' if matches!(chars.peek(), Some((_, '*'))) => {
                    chars.next();
                    result.push_str("/*");
                    state = SqlScanState::BlockComment;
                }
                _ => result.push(ch),
            },
            SqlScanState::SingleQuoted { escaped } => {
                result.push(ch);
                if escaped {
                    state = SqlScanState::SingleQuoted { escaped: false };
                    continue;
                }
                match ch {
                    '\\' => state = SqlScanState::SingleQuoted { escaped: true },
                    '\'' if matches!(chars.peek(), Some((_, '\''))) => {
                        if let Some((_, next)) = chars.next() {
                            result.push(next);
                        }
                    }
                    '\'' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::DoubleQuoted { escaped } => {
                result.push(ch);
                if escaped {
                    state = SqlScanState::DoubleQuoted { escaped: false };
                    continue;
                }
                match ch {
                    '\\' => state = SqlScanState::DoubleQuoted { escaped: true },
                    '"' if matches!(chars.peek(), Some((_, '"'))) => {
                        if let Some((_, next)) = chars.next() {
                            result.push(next);
                        }
                    }
                    '"' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::BacktickQuoted => {
                result.push(ch);
                match ch {
                    '`' if matches!(chars.peek(), Some((_, '`'))) => {
                        if let Some((_, next)) = chars.next() {
                            result.push(next);
                        }
                    }
                    '`' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::LineComment => {
                result.push(ch);
                if matches!(ch, '\n' | '\r') {
                    state = SqlScanState::Code;
                }
            }
            SqlScanState::BlockComment => {
                result.push(ch);
                if ch == '*' && matches!(chars.peek(), Some((_, '/'))) {
                    if let Some((_, next)) = chars.next() {
                        result.push(next);
                    }
                    state = SqlScanState::Code;
                }
            }
        }
    }

    if bind_idx != binds.len() {
        return Err(Error::QueryBuilderError(
            format!(
                "ClickHouse query rendered fewer placeholders ({bind_idx}) than bound values ({})",
                binds.len()
            )
            .into(),
        ));
    }

    Ok(PreparedQuery {
        sql: result,
        params,
    })
}

#[cfg(test)]
fn inline_binds(
    sql: &str,
    metadata: &[ClickHouseTypeMetadata],
    binds: &[Option<Vec<u8>>],
) -> QueryResult<String> {
    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.char_indices().peekable();
    let mut bind_idx = 0;
    let mut state = SqlScanState::Code;

    while let Some((_, ch)) = chars.next() {
        match state {
            SqlScanState::Code => match ch {
                '?' => {
                    if matches!(chars.peek(), Some((_, '?'))) {
                        chars.next();
                        result.push('?');
                    } else {
                        push_bind_literal(&mut result, bind_idx, metadata, binds)?;
                        bind_idx += 1;
                    }
                }
                '\'' => {
                    result.push(ch);
                    state = SqlScanState::SingleQuoted { escaped: false };
                }
                '"' => {
                    result.push(ch);
                    state = SqlScanState::DoubleQuoted { escaped: false };
                }
                '`' => {
                    result.push(ch);
                    state = SqlScanState::BacktickQuoted;
                }
                '-' if matches!(chars.peek(), Some((_, '-'))) => {
                    chars.next();
                    result.push_str("--");
                    state = SqlScanState::LineComment;
                }
                '#' => {
                    result.push(ch);
                    state = SqlScanState::LineComment;
                }
                '/' if matches!(chars.peek(), Some((_, '*'))) => {
                    chars.next();
                    result.push_str("/*");
                    state = SqlScanState::BlockComment;
                }
                _ => result.push(ch),
            },
            SqlScanState::SingleQuoted { escaped } => {
                result.push(ch);
                if escaped {
                    state = SqlScanState::SingleQuoted { escaped: false };
                    continue;
                }
                match ch {
                    '\\' => state = SqlScanState::SingleQuoted { escaped: true },
                    '\'' if matches!(chars.peek(), Some((_, '\''))) => {
                        if let Some((_, next)) = chars.next() {
                            result.push(next);
                        }
                    }
                    '\'' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::DoubleQuoted { escaped } => {
                result.push(ch);
                if escaped {
                    state = SqlScanState::DoubleQuoted { escaped: false };
                    continue;
                }
                match ch {
                    '\\' => state = SqlScanState::DoubleQuoted { escaped: true },
                    '"' if matches!(chars.peek(), Some((_, '"'))) => {
                        if let Some((_, next)) = chars.next() {
                            result.push(next);
                        }
                    }
                    '"' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::BacktickQuoted => {
                result.push(ch);
                match ch {
                    '`' if matches!(chars.peek(), Some((_, '`'))) => {
                        if let Some((_, next)) = chars.next() {
                            result.push(next);
                        }
                    }
                    '`' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::LineComment => {
                result.push(ch);
                if matches!(ch, '\n' | '\r') {
                    state = SqlScanState::Code;
                }
            }
            SqlScanState::BlockComment => {
                result.push(ch);
                if ch == '*' && matches!(chars.peek(), Some((_, '/'))) {
                    if let Some((_, next)) = chars.next() {
                        result.push(next);
                    }
                    state = SqlScanState::Code;
                }
            }
        }
    }

    if bind_idx != binds.len() {
        return Err(Error::QueryBuilderError(
            format!(
                "ClickHouse query rendered fewer placeholders ({bind_idx}) than bound values ({})",
                binds.len()
            )
            .into(),
        ));
    }

    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlScanState {
    Code,
    SingleQuoted { escaped: bool },
    DoubleQuoted { escaped: bool },
    BacktickQuoted,
    LineComment,
    BlockComment,
}

fn push_bind_literal(
    result: &mut String,
    bind_idx: usize,
    metadata: &[ClickHouseTypeMetadata],
    binds: &[Option<Vec<u8>>],
) -> QueryResult<()> {
    let Some(bind) = binds.get(bind_idx) else {
        return Err(Error::QueryBuilderError(
            format!(
                "ClickHouse query rendered more placeholders than bound values ({})",
                binds.len()
            )
            .into(),
        ));
    };

    match bind {
        Some(bytes) if should_quote_bind(metadata.get(bind_idx).map(|m| m.name)) => {
            push_string_literal(result, bytes)?;
        }
        Some(bytes) => result.push_str(std::str::from_utf8(bytes).map_err(|err| {
            Error::SerializationError(Box::new(err) as Box<dyn StdError + Send + Sync>)
        })?),
        None => result.push_str("NULL"),
    }
    Ok(())
}

fn push_bind_parameter_or_literal(
    result: &mut String,
    params: &mut Vec<(String, String)>,
    bind_idx: usize,
    metadata: &[ClickHouseTypeMetadata],
    binds: &[Option<Vec<u8>>],
) -> QueryResult<()> {
    let Some(bind) = binds.get(bind_idx) else {
        return Err(Error::QueryBuilderError(
            format!(
                "ClickHouse query rendered more placeholders than bound values ({})",
                binds.len()
            )
            .into(),
        ));
    };
    let Some(bind_metadata) = metadata.get(bind_idx) else {
        push_bind_literal(result, bind_idx, metadata, binds)?;
        return Ok(());
    };

    let Some(parameter_type) = server_parameter_type(bind_metadata, bind) else {
        push_bind_literal(result, bind_idx, metadata, binds)?;
        return Ok(());
    };
    let parameter_value = server_parameter_value(bind_metadata.name, bind)?;
    let parameter_name = format!("dc_p{bind_idx}");

    result.push('{');
    result.push_str(&parameter_name);
    result.push(':');
    result.push_str(&parameter_type);
    result.push('}');
    params.push((format!("param_{parameter_name}"), parameter_value));
    Ok(())
}

fn server_parameter_type(
    metadata: &ClickHouseTypeMetadata,
    bind: &Option<Vec<u8>>,
) -> Option<String> {
    // ClickHouse 24.8 rejects the documented `NULL` spelling for HTTP query
    // parameters even with `Nullable(T)`, so keep nulls on the proven inline path.
    if bind.is_none() {
        return None;
    }

    let parameter_type = metadata.parameter_type();
    supports_server_parameter_type(parameter_type).then(|| parameter_type.to_owned())
}

fn supports_server_parameter_type(parameter_type: &str) -> bool {
    !matches!(
        parameter_type,
        "AggregateFunction"
            | "Array"
            | "Decimal"
            | "LowCardinality"
            | "Map"
            | "Nested"
            | "Tuple"
            | "Variant"
    )
}

fn server_parameter_value(
    metadata_name: &'static str,
    bind: &Option<Vec<u8>>,
) -> QueryResult<String> {
    let Some(bytes) = bind else {
        return Ok("NULL".to_owned());
    };
    let value = std::str::from_utf8(bytes).map_err(|err| {
        Error::SerializationError(Box::new(err) as Box<dyn StdError + Send + Sync>)
    })?;

    if should_escape_server_parameter(metadata_name) {
        Ok(escape_server_parameter_string(value))
    } else {
        Ok(value.to_owned())
    }
}

fn should_escape_server_parameter(metadata_name: &'static str) -> bool {
    matches!(
        metadata_name,
        "String"
            | "Date"
            | "DateTime"
            | "DateTime64"
            | "UUID"
            | "IPv4"
            | "IPv6"
            | "JSON"
            | "Dynamic"
    )
}

fn escape_server_parameter_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' | '\'' | '`' | '\t' | '\n' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn should_quote_bind(metadata_name: Option<&str>) -> bool {
    matches!(
        metadata_name,
        Some("String" | "Date" | "DateTime" | "DateTime64" | "UUID" | "IPv4" | "IPv6" | "JSON")
    )
}

fn push_string_literal(result: &mut String, bytes: &[u8]) -> QueryResult<()> {
    let value = std::str::from_utf8(bytes).map_err(|err| {
        Error::SerializationError(Box::new(err) as Box<dyn StdError + Send + Sync>)
    })?;
    result.push('\'');
    for ch in value.chars() {
        match ch {
            '\0' => result.push_str("\\0"),
            '\\' => result.push_str("\\\\"),
            '\'' => result.push_str("\\'"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            other => result.push(other),
        }
    }
    result.push('\'');
    Ok(())
}

fn escape_clickhouse_client_template(sql: &str) -> Cow<'_, str> {
    if sql.contains('?') {
        Cow::Owned(sql.replace('?', "??"))
    } else {
        Cow::Borrowed(sql)
    }
}

fn parse_rows(bytes: &[u8]) -> QueryResult<Vec<ClickHouseRow>> {
    let text = std::str::from_utf8(bytes).map_err(|err| {
        Error::DeserializationError(Box::new(err) as Box<dyn StdError + Send + Sync>)
    })?;
    let mut lines = text.lines();
    let Some(names_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let _types_line = lines.next().ok_or_else(|| {
        Error::DeserializationError("ClickHouse response did not include a type header".into())
    })?;

    let names = split_tsv_line(names_line)
        .into_iter()
        .map(|field| decode_tsv_field(field).map(|value| value.unwrap_or_default()))
        .map(|result| {
            result.and_then(|bytes| {
                String::from_utf8(bytes).map_err(|err| {
                    Error::DeserializationError(Box::new(err) as Box<dyn StdError + Send + Sync>)
                })
            })
        })
        .collect::<QueryResult<Vec<_>>>()?;

    let by_name = names
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.clone(), idx))
        .collect();
    let header = Arc::new(RowHeader { names, by_name });

    let mut rows = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let values = split_tsv_line(line)
            .into_iter()
            .map(decode_tsv_field)
            .collect::<QueryResult<Vec<_>>>()?;
        if values.len() != header.names.len() {
            return Err(Error::DeserializationError(
                format!(
                    "ClickHouse row had {} fields but header declared {}",
                    values.len(),
                    header.names.len()
                )
                .into(),
            ));
        }

        rows.push(ClickHouseRow {
            header: Arc::clone(&header),
            values,
        });
    }

    Ok(rows)
}

fn split_tsv_line(line: &str) -> Vec<&[u8]> {
    line.as_bytes().split(|byte| *byte == b'\t').collect()
}

fn decode_tsv_field(field: &[u8]) -> QueryResult<Option<Vec<u8>>> {
    if field == b"\\N" {
        return Ok(None);
    }

    let mut decoded = Vec::with_capacity(field.len());
    let mut idx = 0;
    while idx < field.len() {
        if field[idx] != b'\\' {
            decoded.push(field[idx]);
            idx += 1;
            continue;
        }

        idx += 1;
        let Some(escaped) = field.get(idx).copied() else {
            decoded.push(b'\\');
            break;
        };
        match escaped {
            b'0' => decoded.push(0),
            b'b' => decoded.push(8),
            b'f' => decoded.push(12),
            b'n' => decoded.push(b'\n'),
            b'r' => decoded.push(b'\r'),
            b't' => decoded.push(b'\t'),
            b'\\' => decoded.push(b'\\'),
            other => decoded.push(other),
        }
        idx += 1;
    }

    Ok(Some(decoded))
}

fn split_statements(query: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut chars = query.char_indices().peekable();
    let mut state = SqlScanState::Code;

    while let Some((idx, ch)) = chars.next() {
        match state {
            SqlScanState::Code => match ch {
                ';' => {
                    push_statement(&mut statements, &query[start..idx]);
                    start = idx + ch.len_utf8();
                }
                '\'' => state = SqlScanState::SingleQuoted { escaped: false },
                '"' => state = SqlScanState::DoubleQuoted { escaped: false },
                '`' => state = SqlScanState::BacktickQuoted,
                '-' if matches!(chars.peek(), Some((_, '-'))) => {
                    chars.next();
                    state = SqlScanState::LineComment;
                }
                '#' => state = SqlScanState::LineComment,
                '/' if matches!(chars.peek(), Some((_, '*'))) => {
                    chars.next();
                    state = SqlScanState::BlockComment;
                }
                _ => {}
            },
            SqlScanState::SingleQuoted { escaped } => {
                if escaped {
                    state = SqlScanState::SingleQuoted { escaped: false };
                    continue;
                }
                match ch {
                    '\\' => state = SqlScanState::SingleQuoted { escaped: true },
                    '\'' if matches!(chars.peek(), Some((_, '\''))) => {
                        chars.next();
                    }
                    '\'' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::DoubleQuoted { escaped } => {
                if escaped {
                    state = SqlScanState::DoubleQuoted { escaped: false };
                    continue;
                }
                match ch {
                    '\\' => state = SqlScanState::DoubleQuoted { escaped: true },
                    '"' if matches!(chars.peek(), Some((_, '"'))) => {
                        chars.next();
                    }
                    '"' => state = SqlScanState::Code,
                    _ => {}
                }
            }
            SqlScanState::BacktickQuoted => match ch {
                '`' if matches!(chars.peek(), Some((_, '`'))) => {
                    chars.next();
                }
                '`' => state = SqlScanState::Code,
                _ => {}
            },
            SqlScanState::LineComment => {
                if matches!(ch, '\n' | '\r') {
                    state = SqlScanState::Code;
                }
            }
            SqlScanState::BlockComment => {
                if ch == '*' && matches!(chars.peek(), Some((_, '/'))) {
                    chars.next();
                    state = SqlScanState::Code;
                }
            }
        }
    }

    push_statement(&mut statements, &query[start..]);
    statements
}

fn push_statement<'a>(statements: &mut Vec<&'a str>, statement: &'a str) {
    let statement = statement.trim();
    if !statement.is_empty() && statement_has_code(statement) {
        statements.push(statement);
    }
}

fn statement_has_code(statement: &str) -> bool {
    let mut chars = statement.char_indices().peekable();
    let mut state = SqlScanState::Code;

    while let Some((_, ch)) = chars.next() {
        match state {
            SqlScanState::Code => match ch {
                ch if ch.is_whitespace() => {}
                '-' if matches!(chars.peek(), Some((_, '-'))) => {
                    chars.next();
                    state = SqlScanState::LineComment;
                }
                '#' => state = SqlScanState::LineComment,
                '/' if matches!(chars.peek(), Some((_, '*'))) => {
                    chars.next();
                    state = SqlScanState::BlockComment;
                }
                _ => return true,
            },
            SqlScanState::LineComment => {
                if matches!(ch, '\n' | '\r') {
                    state = SqlScanState::Code;
                }
            }
            SqlScanState::BlockComment => {
                if ch == '*' && matches!(chars.peek(), Some((_, '/'))) {
                    chars.next();
                    state = SqlScanState::Code;
                }
            }
            SqlScanState::SingleQuoted { .. }
            | SqlScanState::DoubleQuoted { .. }
            | SqlScanState::BacktickQuoted => return true,
        }
    }

    false
}

fn unsupported_transactions() -> Error {
    Error::QueryBuilderError(
        "ClickHouse transactions are not supported by diesel-clickhouse".into(),
    )
}

fn clickhouse_error(err: clickhouse::error::Error) -> Error {
    Error::DatabaseError(DatabaseErrorKind::Unknown, Box::new(err.to_string()))
}

impl fmt::Debug for AsyncClickHouseConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncClickHouseConnection")
            .field("transaction_state", &self.transaction_state)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::ClickHouseTypeMetadata;

    #[test]
    fn inline_binds_ignores_question_marks_in_literals_and_comments() {
        let rendered = inline_binds(
            "SELECT '?' AS literal, ? AS bound -- ? in comment\n, 'still ?' AS tail",
            &[ClickHouseTypeMetadata::new("String")],
            &[Some(b"tenant's value".to_vec())],
        )
        .expect("bind inlining should succeed");

        assert_eq!(
            rendered,
            "SELECT '?' AS literal, 'tenant\\'s value' AS bound -- ? in comment\n, 'still ?' AS tail"
        );
    }

    #[test]
    fn inline_binds_preserves_escaped_question_marks() {
        let rendered = inline_binds(
            "SELECT ?? AS literal_question, ? AS bound",
            &[ClickHouseTypeMetadata::new("Int32")],
            &[Some(b"42".to_vec())],
        )
        .expect("bind inlining should succeed");

        assert_eq!(rendered, "SELECT ? AS literal_question, 42 AS bound");
    }

    #[test]
    fn inline_binds_rejects_unbound_placeholders() {
        let err = inline_binds("SELECT ?", &[], &[]).expect_err("placeholder should need a bind");
        assert!(
            err.to_string()
                .contains("rendered more placeholders than bound values"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn options_from_url_extracts_credentials_database_and_settings() {
        let options = ClickHouseConnectionOptions::from_url(
            "http://url_user:url_password@example.com:8123/path_db?database=query_db&max_threads=1",
        )
        .expect("url should parse");

        assert_eq!(options.url, "http://example.com:8123/");
        assert_eq!(options.user.as_deref(), Some("url_user"));
        assert_eq!(options.password.as_deref(), Some("url_password"));
        assert_eq!(options.database.as_deref(), Some("query_db"));
        assert_eq!(
            options.options.get("max_threads").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn options_builder_sets_explicit_fields() {
        let options = ClickHouseConnectionOptions::new("http://example.com:8123")
            .user("default")
            .password("secret")
            .database("analytics")
            .option("max_threads", "1")
            .option("max_threads", "2");

        assert_eq!(options.url, "http://example.com:8123");
        assert_eq!(options.user.as_deref(), Some("default"));
        assert_eq!(options.password.as_deref(), Some("secret"));
        assert_eq!(options.database.as_deref(), Some("analytics"));
        assert_eq!(
            options.options.get("max_threads").map(String::as_str),
            Some("2")
        );
    }

    #[test]
    fn parameterize_binds_uses_clickhouse_server_parameters() {
        let prepared = parameterize_binds(
            "SELECT '?' AS literal, ? AS name, ? AS age",
            &[
                ClickHouseTypeMetadata::new("String"),
                ClickHouseTypeMetadata::new("Int32"),
            ],
            &[Some(b"O'Reilly?".to_vec()), Some(b"42".to_vec())],
        )
        .expect("parameterization should succeed");

        assert_eq!(
            prepared.sql,
            "SELECT '?' AS literal, {dc_p0:String} AS name, {dc_p1:Int32} AS age"
        );
        assert_eq!(
            prepared.params,
            vec![
                ("param_dc_p0".to_string(), "O\\'Reilly?".to_string()),
                ("param_dc_p1".to_string(), "42".to_string()),
            ]
        );
    }

    #[test]
    fn array_type_metadata_includes_element_type_for_parameters() {
        let mut lookup = ();
        let metadata = <ClickHouse as diesel::sql_types::HasSqlType<
            crate::types::Array<crate::types::UInt64>,
        >>::metadata(&mut lookup);

        assert_eq!(metadata.name, "Array");
        assert_eq!(metadata.parameter_type(), "Array(UInt64)");
    }

    #[test]
    fn parameterize_binds_uses_typed_array_server_parameters() {
        let prepared = parameterize_binds(
            "SELECT has(?, toUInt64(2))",
            &[ClickHouseTypeMetadata::with_parameter_type(
                "Array",
                "Array(UInt64)",
            )],
            &[Some(b"[1,2,3]".to_vec())],
        )
        .expect("parameterization should succeed");

        assert_eq!(
            prepared.sql,
            "SELECT has({dc_p0:Array(UInt64)}, toUInt64(2))"
        );
        assert_eq!(
            prepared.params,
            vec![("param_dc_p0".to_string(), "[1,2,3]".to_string())]
        );
    }

    #[test]
    fn parameterize_binds_rejects_unbound_placeholders() {
        let err =
            parameterize_binds("SELECT ?", &[], &[]).expect_err("placeholder should need a bind");

        assert!(
            err.to_string()
                .contains("rendered more placeholders than bound values"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parameterize_binds_rejects_unused_binds() {
        let err = parameterize_binds(
            "SELECT '?' AS literal",
            &[ClickHouseTypeMetadata::new("String")],
            &[Some(b"unused".to_vec())],
        )
        .expect_err("unused bind should be rejected");

        assert!(
            err.to_string()
                .contains("rendered fewer placeholders (0) than bound values (1)"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parameterize_binds_inlines_nulls() {
        let prepared =
            parameterize_binds("SELECT ?", &[ClickHouseTypeMetadata::new("Int32")], &[None])
                .expect("parameterization should succeed");

        assert_eq!(prepared.sql, "SELECT NULL");
        assert!(prepared.params.is_empty());
    }

    #[test]
    fn parameterize_binds_inlines_abstract_parameter_types() {
        let prepared = parameterize_binds(
            "SELECT ?",
            &[ClickHouseTypeMetadata::new("Decimal")],
            &[Some(b"123.45".to_vec())],
        )
        .expect("fallback inlining should succeed");

        assert_eq!(prepared.sql, "SELECT 123.45");
        assert!(prepared.params.is_empty());
    }

    #[test]
    fn escape_clickhouse_client_template_doubles_literal_question_marks() {
        assert_eq!(
            escape_clickhouse_client_template("SELECT '?' AS literal, ? AS raw").as_ref(),
            "SELECT '??' AS literal, ?? AS raw"
        );
    }

    #[test]
    fn split_statements_respects_literals_and_comments() {
        let statements = split_statements(
            "SELECT ';' AS semi; -- comment ;\nSELECT 'it\\'s; ok'; /* block ; */ SELECT `semi;id`",
        );

        assert_eq!(
            statements,
            vec![
                "SELECT ';' AS semi",
                "-- comment ;\nSELECT 'it\\'s; ok'",
                "/* block ; */ SELECT `semi;id`",
            ]
        );
    }

    #[test]
    fn split_statements_filters_comment_only_segments() {
        assert!(split_statements("-- comment ;\n/* block ; */").is_empty());
    }
}
