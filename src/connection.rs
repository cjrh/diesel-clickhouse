//! Synchronous Diesel connection backed by ClickHouse's HTTP interface.
//!
//! The connection intentionally models ClickHouse as ClickHouse: transactions are
//! reported as unsupported, statement execution returns no affected-row count,
//! and result loading uses ClickHouse's `TabSeparatedWithNamesAndTypes` format as
//! a simple transport for Diesel's row deserializer.

use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::ops::Range;

use diesel::connection::{
    Connection, ConnectionSealed, DefaultLoadingMode, Instrumentation, InstrumentationEvent,
    LoadConnection, SimpleConnection, StrQueryHelper, TransactionManager, TransactionManagerStatus,
    WithMetadataLookup, get_default_instrumentation,
};
use diesel::expression::QueryMetadata;
use diesel::query_builder::{
    Query, QueryBuilder, QueryFragment, QueryId, bind_collector::RawBytesBindCollector,
};
use diesel::result::{ConnectionError, ConnectionResult, DatabaseErrorKind, Error, QueryResult};
use diesel::row::{Field, PartialRow, Row, RowIndex, RowSealed};
use tokio::runtime::{Builder, Runtime};

use crate::backend::{ClickHouse, ClickHouseQueryBuilder};

/// A synchronous Diesel connection for ClickHouse over HTTP.
///
/// This is the first connection implementation spike. It supports idiomatic
/// Diesel loading for primitive, text, nullable, and common composite result values while keeping
/// ClickHouse-specific semantics explicit: transactions are unsupported and
/// command execution reports `0` affected rows because ClickHouse's HTTP
/// interface does not provide OLTP-style row counts for DDL/mutations.
#[allow(missing_debug_implementations)]
pub struct ClickHouseConnection {
    client: clickhouse::Client,
    runtime: Runtime,
    transaction_state: TransactionManagerStatus,
    metadata_lookup: (),
    instrumentation: Option<Box<dyn Instrumentation>>,
}

impl ClickHouseConnection {
    /// Build a Diesel connection around an already-configured ClickHouse client.
    pub fn with_client(client: clickhouse::Client) -> ConnectionResult<Self> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| ConnectionError::BadConnection(err.to_string()))?;

        Ok(Self {
            client,
            runtime,
            transaction_state: TransactionManagerStatus::default(),
            metadata_lookup: (),
            instrumentation: get_default_instrumentation(),
        })
    }

    /// Access the underlying ClickHouse client for ClickHouse-specific setup.
    pub fn client(&self) -> &clickhouse::Client {
        &self.client
    }

    fn render_query<T>(&mut self, source: &T) -> QueryResult<String>
    where
        T: QueryFragment<ClickHouse> + QueryId,
    {
        let backend = ClickHouse;
        let mut query_builder = ClickHouseQueryBuilder::default();
        source.to_sql(&mut query_builder, &backend)?;
        let sql = query_builder.finish();

        let mut bind_collector = RawBytesBindCollector::<ClickHouse>::new();
        source.collect_binds(&mut bind_collector, &mut self.metadata_lookup, &backend)?;
        inline_binds(&sql, &bind_collector.metadata, &bind_collector.binds)
    }

    fn execute_sql(&mut self, sql: &str) -> QueryResult<()> {
        let query = StrQueryHelper::new(sql);
        self.instrumentation
            .on_connection_event(InstrumentationEvent::start_query(&query));

        let client_sql = escape_clickhouse_client_template(sql);
        let result = self
            .runtime
            .block_on(self.client.query(&client_sql).execute())
            .map_err(clickhouse_error);

        self.instrumentation
            .on_connection_event(InstrumentationEvent::finish_query(
                &query,
                result.as_ref().err(),
            ));
        result
    }

    fn load_sql(&mut self, sql: &str) -> QueryResult<Vec<ClickHouseRow>> {
        let query = StrQueryHelper::new(sql);
        self.instrumentation
            .on_connection_event(InstrumentationEvent::start_query(&query));

        let client_sql = escape_clickhouse_client_template(sql);
        let result = self.runtime.block_on(async {
            let mut cursor = self
                .client
                .query(&client_sql)
                .fetch_bytes("TabSeparatedWithNamesAndTypes")
                .map_err(clickhouse_error)?;
            cursor.collect().await.map_err(clickhouse_error)
        });

        let result = result.and_then(|bytes| parse_rows(&bytes));
        self.instrumentation
            .on_connection_event(InstrumentationEvent::finish_query(
                &query,
                result.as_ref().err(),
            ));
        result
    }
}

impl SimpleConnection for ClickHouseConnection {
    fn batch_execute(&mut self, query: &str) -> QueryResult<()> {
        for statement in split_statements(query) {
            self.execute_sql(statement)?;
        }
        Ok(())
    }
}

impl ConnectionSealed for ClickHouseConnection {}

impl Connection for ClickHouseConnection {
    type Backend = ClickHouse;
    type TransactionManager = ClickHouseTransactionManager;

    fn establish(database_url: &str) -> ConnectionResult<Self> {
        let mut instrumentation = get_default_instrumentation();
        instrumentation.on_connection_event(InstrumentationEvent::start_establish_connection(
            database_url,
        ));

        let result = establish(database_url);

        instrumentation.on_connection_event(InstrumentationEvent::finish_establish_connection(
            database_url,
            result.as_ref().err(),
        ));

        let mut conn = result?;
        conn.instrumentation = instrumentation;
        Ok(conn)
    }

    fn execute_returning_count<T>(&mut self, source: &T) -> QueryResult<usize>
    where
        T: QueryFragment<Self::Backend> + QueryId,
    {
        let sql = self.render_query(source)?;
        self.execute_sql(&sql)?;
        Ok(0)
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
}

impl LoadConnection<DefaultLoadingMode> for ClickHouseConnection {
    type Cursor<'conn, 'query>
        = ClickHouseCursor
    where
        Self: 'conn;

    type Row<'conn, 'query>
        = ClickHouseRow
    where
        Self: 'conn;

    fn load<'conn, 'query, T>(
        &'conn mut self,
        source: T,
    ) -> QueryResult<Self::Cursor<'conn, 'query>>
    where
        T: Query + QueryFragment<Self::Backend> + QueryId + 'query,
        Self::Backend: QueryMetadata<T::SqlType>,
    {
        let sql = self.render_query(&source)?;
        let rows = self.load_sql(&sql)?;
        Ok(rows.into_iter().map(Ok).collect::<Vec<_>>().into_iter())
    }
}

impl WithMetadataLookup for ClickHouseConnection {
    fn metadata_lookup(
        &mut self,
    ) -> &mut <Self::Backend as diesel::sql_types::TypeMetadata>::MetadataLookup {
        &mut self.metadata_lookup
    }
}

/// Transaction manager that makes unsupported ClickHouse transactions explicit.
#[derive(Debug, Default)]
pub struct ClickHouseTransactionManager;

impl TransactionManager<ClickHouseConnection> for ClickHouseTransactionManager {
    type TransactionStateData = TransactionManagerStatus;

    fn begin_transaction(_conn: &mut ClickHouseConnection) -> QueryResult<()> {
        Err(unsupported_transactions())
    }

    fn rollback_transaction(_conn: &mut ClickHouseConnection) -> QueryResult<()> {
        Err(unsupported_transactions())
    }

    fn commit_transaction(_conn: &mut ClickHouseConnection) -> QueryResult<()> {
        Err(unsupported_transactions())
    }

    fn transaction_manager_status_mut(
        conn: &mut ClickHouseConnection,
    ) -> &mut TransactionManagerStatus {
        &mut conn.transaction_state
    }
}

/// Iterator returned by [`ClickHouseConnection`] load operations.
pub type ClickHouseCursor = std::vec::IntoIter<QueryResult<ClickHouseRow>>;

/// Owned result row used by the ClickHouse connection.
#[derive(Debug, Clone)]
pub struct ClickHouseRow {
    fields: Vec<ClickHouseFieldValue>,
    fields_by_name: HashMap<String, usize>,
}

/// Field view returned from [`ClickHouseRow`].
#[derive(Debug, Clone, Copy)]
pub struct ClickHouseField<'a> {
    inner: &'a ClickHouseFieldValue,
}

#[derive(Debug, Clone)]
struct ClickHouseFieldValue {
    name: String,
    value: Option<Vec<u8>>,
}

impl RowSealed for ClickHouseRow {}

impl RowIndex<usize> for ClickHouseRow {
    fn idx(&self, idx: usize) -> Option<usize> {
        (idx < self.fields.len()).then_some(idx)
    }
}

impl RowIndex<&str> for ClickHouseRow {
    fn idx(&self, idx: &str) -> Option<usize> {
        self.fields_by_name.get(idx).copied()
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
        self.fields.len()
    }

    fn get<'b, I>(&'b self, idx: I) -> Option<Self::Field<'b>>
    where
        'a: 'b,
        Self: RowIndex<I>,
    {
        let idx = self.idx(idx)?;
        self.fields.get(idx).map(|inner| ClickHouseField { inner })
    }

    fn partial_row(&self, range: Range<usize>) -> PartialRow<'_, Self::InnerPartialRow> {
        PartialRow::new(self, range)
    }
}

impl<'a> Field<'a, ClickHouse> for ClickHouseField<'a> {
    fn field_name(&self) -> Option<&str> {
        Some(&self.inner.name)
    }

    fn value(&self) -> Option<<ClickHouse as diesel::backend::Backend>::RawValue<'_>> {
        self.inner.value.as_deref()
    }
}

fn establish(database_url: &str) -> ConnectionResult<ClickHouseConnection> {
    let parsed = url::Url::parse(database_url)
        .map_err(|err| ConnectionError::InvalidConnectionUrl(err.to_string()))?;

    let mut client = clickhouse::Client::default()
        .with_url(base_url(&parsed)?)
        .with_product_info("diesel-clickhouse", env!("CARGO_PKG_VERSION"));

    if !parsed.username().is_empty() {
        client = client.with_user(parsed.username());
    }
    if let Some(password) = parsed.password() {
        client = client.with_password(password);
    }

    let mut database_from_query = None;
    for (name, value) in parsed.query_pairs() {
        match name.as_ref() {
            "user" if parsed.username().is_empty() => client = client.with_user(value.as_ref()),
            "password" if parsed.password().is_none() => {
                client = client.with_password(value.as_ref());
            }
            "database" => database_from_query = Some(value.into_owned()),
            option => client = client.with_option(option, value.as_ref()),
        }
    }

    if let Some(database) = database_from_query.or_else(|| database_from_path(&parsed)) {
        client = client.with_database(database);
    }

    let mut conn = ClickHouseConnection::with_client(client)?;
    conn.execute_sql("SELECT 1 FORMAT Null")
        .map_err(ConnectionError::CouldntSetupConfiguration)?;
    Ok(conn)
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

fn inline_binds(
    sql: &str,
    metadata: &[crate::backend::ClickHouseTypeMetadata],
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
    metadata: &[crate::backend::ClickHouseTypeMetadata],
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

    let mut rows = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let values = split_tsv_line(line)
            .into_iter()
            .map(decode_tsv_field)
            .collect::<QueryResult<Vec<_>>>()?;
        if values.len() != names.len() {
            return Err(Error::DeserializationError(
                format!(
                    "ClickHouse row had {} fields but header declared {}",
                    values.len(),
                    names.len()
                )
                .into(),
            ));
        }

        let fields = names
            .iter()
            .cloned()
            .zip(values)
            .map(|(name, value)| ClickHouseFieldValue { name, value })
            .collect::<Vec<_>>();
        let fields_by_name = fields
            .iter()
            .enumerate()
            .map(|(idx, field)| (field.name.clone(), idx))
            .collect();
        rows.push(ClickHouseRow {
            fields,
            fields_by_name,
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

impl fmt::Debug for ClickHouseConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClickHouseConnection")
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
