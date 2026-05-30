//! Builders for ClickHouse DDL statements that Diesel does not model.
//!
//! These fragments are intentionally small and explicit: ClickHouse DDL has a
//! large surface area, so the builder focuses on common table creation while
//! leaving escape hatches (`raw` expressions and custom data types/engines) for
//! features not yet represented as structured Rust values.

use diesel::query_builder::{AstPass, QueryFragment, QueryId};
use diesel::result::{Error, QueryResult};

use crate::backend::ClickHouse;

/// Start a `CREATE TABLE` statement.
pub fn create_table(name: impl Into<String>) -> CreateTable {
    CreateTable {
        name: name.into(),
        if_not_exists: false,
        columns: Vec::new(),
        indexes: Vec::new(),
        projections: Vec::new(),
        engine: None,
    }
}

/// Build a ClickHouse table projection definition.
pub fn projection(name: impl Into<String>, body: impl Into<String>) -> TableProjection {
    TableProjection::new(name, body)
}

/// Build a ClickHouse `vector_similarity` skipping index.
pub fn vector_similarity_index(
    name: impl Into<String>,
    expression: impl Into<String>,
    dimensions: u64,
) -> TableIndex {
    TableIndex::vector_similarity(name, expression, dimensions)
}

/// Start a `CREATE MATERIALIZED VIEW` statement.
pub fn create_materialized_view(name: impl Into<String>) -> CreateMaterializedViewBuilder {
    CreateMaterializedViewBuilder {
        name: name.into(),
        if_not_exists: false,
        target: None,
        engine: None,
        populate: false,
    }
}

/// Start an `ALTER TABLE` statement.
pub fn alter_table(name: impl Into<String>) -> AlterTable {
    AlterTable {
        name: name.into(),
        operation: None,
        settings: Vec::new(),
    }
}

/// Start a `MergeTree` engine definition.
pub fn merge_tree() -> MergeTree {
    MergeTree::new(MergeTreeKind::Base)
}

/// Start a `ReplacingMergeTree` engine definition.
pub fn replacing_merge_tree() -> MergeTree {
    MergeTree::new(MergeTreeKind::Replacing { version: None })
}

/// Start a `ReplacingMergeTree(version)` engine definition.
pub fn replacing_merge_tree_with(version: impl Into<String>) -> MergeTree {
    MergeTree::new(MergeTreeKind::Replacing {
        version: Some(version.into()),
    })
}

/// Start a `SummingMergeTree` engine definition.
pub fn summing_merge_tree() -> MergeTree {
    MergeTree::new(MergeTreeKind::Summing { columns: None })
}

/// Start a `SummingMergeTree((columns...))` engine definition.
pub fn summing_merge_tree_with<I, S>(columns: I) -> MergeTree
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    MergeTree::new(MergeTreeKind::Summing {
        columns: Some(columns.into_iter().map(Into::into).collect()),
    })
}

/// Start an `AggregatingMergeTree` engine definition.
pub fn aggregating_merge_tree() -> MergeTree {
    MergeTree::new(MergeTreeKind::Aggregating)
}

/// Start a `CollapsingMergeTree(sign)` engine definition.
pub fn collapsing_merge_tree(sign: impl Into<String>) -> MergeTree {
    MergeTree::new(MergeTreeKind::Collapsing { sign: sign.into() })
}

/// Start a `VersionedCollapsingMergeTree(sign, version)` engine definition.
pub fn versioned_collapsing_merge_tree(
    sign: impl Into<String>,
    version: impl Into<String>,
) -> MergeTree {
    MergeTree::new(MergeTreeKind::VersionedCollapsing {
        sign: sign.into(),
        version: version.into(),
    })
}

/// Build a `Distributed(cluster, database, table)` engine definition.
pub fn distributed(
    cluster: impl Into<String>,
    database: impl Into<String>,
    table: impl Into<String>,
) -> DistributedEngine {
    DistributedEngine::new(cluster, database, table)
}

/// Build a `Buffer(...)` engine definition.
#[allow(clippy::too_many_arguments)]
pub fn buffer(
    database: impl Into<String>,
    table: impl Into<String>,
    num_layers: u64,
    min_time: u64,
    max_time: u64,
    min_rows: u64,
    max_rows: u64,
    min_bytes: u64,
    max_bytes: u64,
) -> BufferEngine {
    BufferEngine::new(
        database, table, num_layers, min_time, max_time, min_rows, max_rows, min_bytes, max_bytes,
    )
}

/// `CREATE TABLE ...` statement.
#[derive(Debug, Clone)]
pub struct CreateTable {
    name: String,
    if_not_exists: bool,
    columns: Vec<Column>,
    indexes: Vec<TableIndex>,
    projections: Vec<TableProjection>,
    engine: Option<TableEngine>,
}

/// Builder for `CREATE MATERIALIZED VIEW` before the `AS SELECT` query is known.
#[derive(Debug, Clone)]
pub struct CreateMaterializedViewBuilder {
    name: String,
    if_not_exists: bool,
    target: Option<String>,
    engine: Option<TableEngine>,
    populate: bool,
}

/// `CREATE MATERIALIZED VIEW ... AS SELECT ...` statement.
#[derive(Debug, Clone)]
pub struct CreateMaterializedView<Query> {
    name: String,
    if_not_exists: bool,
    target: Option<String>,
    engine: Option<TableEngine>,
    populate: bool,
    query: Query,
}

/// `ALTER TABLE ...` statement with one operation.
#[derive(Debug, Clone)]
pub struct AlterTable {
    name: String,
    operation: Option<AlterTableOperation>,
    settings: Vec<EngineSetting>,
}

#[derive(Debug, Clone)]
enum AlterTableOperation {
    AddColumn {
        column: Column,
        after: Option<String>,
    },
    DropColumn {
        name: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    AddIndex(TableIndex),
    DropIndex {
        name: String,
    },
    MaterializeIndex {
        name: String,
    },
    AddProjection(TableProjection),
    DropProjection {
        name: String,
    },
    MaterializeProjection {
        name: String,
    },
    ModifyTtl {
        expr: String,
    },
}

impl CreateTable {
    /// Add `IF NOT EXISTS`.
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }

    /// Add a column definition.
    pub fn column(mut self, name: impl Into<String>, data_type: DataType) -> Self {
        self.columns.push(Column::new(name, data_type));
        self
    }

    /// Add a pre-built column definition.
    pub fn column_def(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// Add a ClickHouse table index definition.
    pub fn index(mut self, index: TableIndex) -> Self {
        self.indexes.push(index);
        self
    }

    /// Add a ClickHouse table projection definition.
    pub fn projection(mut self, projection: TableProjection) -> Self {
        self.projections.push(projection);
        self
    }

    /// Set the table engine.
    pub fn engine(mut self, engine: impl Into<TableEngine>) -> Self {
        self.engine = Some(engine.into());
        self
    }
}

impl CreateMaterializedViewBuilder {
    /// Add `IF NOT EXISTS`.
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }

    /// Add `TO target_table` so ClickHouse writes view output into an existing table.
    pub fn to(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Add an inline `ENGINE = ...` for a materialized view that owns its storage.
    pub fn engine(mut self, engine: impl Into<TableEngine>) -> Self {
        self.engine = Some(engine.into());
        self
    }

    /// Add ClickHouse's `POPULATE` modifier.
    pub fn populate(mut self) -> Self {
        self.populate = true;
        self
    }

    /// Finish the materialized view with its source query.
    pub fn as_select<Query>(self, query: Query) -> CreateMaterializedView<Query> {
        CreateMaterializedView {
            name: self.name,
            if_not_exists: self.if_not_exists,
            target: self.target,
            engine: self.engine,
            populate: self.populate,
            query,
        }
    }
}

impl AlterTable {
    /// Add `ADD COLUMN column`.
    pub fn add_column(mut self, column: Column) -> Self {
        self.operation = Some(AlterTableOperation::AddColumn {
            column,
            after: None,
        });
        self
    }

    /// Add `ADD COLUMN column AFTER after_column`.
    pub fn add_column_after(mut self, column: Column, after: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::AddColumn {
            column,
            after: Some(after.into()),
        });
        self
    }

    /// Add `DROP COLUMN name`.
    pub fn drop_column(mut self, name: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::DropColumn { name: name.into() });
        self
    }

    /// Add `RENAME COLUMN from TO to`.
    pub fn rename_column(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::RenameColumn {
            from: from.into(),
            to: to.into(),
        });
        self
    }

    /// Add `ADD INDEX ...`.
    pub fn add_index(mut self, index: TableIndex) -> Self {
        self.operation = Some(AlterTableOperation::AddIndex(index));
        self
    }

    /// Add `DROP INDEX name`.
    pub fn drop_index(mut self, name: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::DropIndex { name: name.into() });
        self
    }

    /// Add `MATERIALIZE INDEX name`.
    pub fn materialize_index(mut self, name: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::MaterializeIndex { name: name.into() });
        self
    }

    /// Add `MODIFY TTL expr`.
    pub fn modify_ttl(mut self, expr: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::ModifyTtl { expr: expr.into() });
        self
    }

    /// Add `ADD PROJECTION ...`.
    pub fn add_projection(mut self, projection: TableProjection) -> Self {
        self.operation = Some(AlterTableOperation::AddProjection(projection));
        self
    }

    /// Add `DROP PROJECTION name`.
    pub fn drop_projection(mut self, name: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::DropProjection { name: name.into() });
        self
    }

    /// Add `MATERIALIZE PROJECTION name`.
    pub fn materialize_projection(mut self, name: impl Into<String>) -> Self {
        self.operation = Some(AlterTableOperation::MaterializeProjection { name: name.into() });
        self
    }

    /// Append `SETTINGS name = value` after the ALTER operation.
    pub fn setting(
        mut self,
        name: impl Into<String>,
        value: impl Into<EngineSettingValue>,
    ) -> Self {
        self.settings.push(EngineSetting {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

/// One ClickHouse column definition.
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    data_type: DataType,
    default: Option<ColumnDefault>,
    codec: Option<String>,
}

impl Column {
    /// Create a column definition.
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            default: None,
            codec: None,
        }
    }

    /// Add `DEFAULT expr`.
    pub fn default_expr(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(ColumnDefault::Default(expr.into()));
        self
    }

    /// Add `MATERIALIZED expr`.
    pub fn materialized_expr(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(ColumnDefault::Materialized(expr.into()));
        self
    }

    /// Add `ALIAS expr`.
    pub fn alias_expr(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(ColumnDefault::Alias(expr.into()));
        self
    }

    /// Add `CODEC(...)`. Pass only the codec expression, e.g. `"ZSTD(1)"`.
    pub fn codec(mut self, codec: impl Into<String>) -> Self {
        self.codec = Some(codec.into());
        self
    }
}

#[derive(Debug, Clone)]
enum ColumnDefault {
    Default(String),
    Materialized(String),
    Alias(String),
}

/// One `PROJECTION name (SELECT ...)` definition in `CREATE TABLE` or `ALTER TABLE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableProjection {
    name: String,
    body: String,
}

impl TableProjection {
    /// Create a projection from its name and the body inside the parentheses.
    ///
    /// Pass the body exactly as ClickHouse expects it, typically a `SELECT ...`
    /// fragment such as `"SELECT tenant_id, count() GROUP BY tenant_id"`.
    pub fn new(name: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            body: body.into(),
        }
    }
}

/// One `INDEX name expr TYPE ... [GRANULARITY n]` definition in `CREATE TABLE`.
#[derive(Debug, Clone)]
pub struct TableIndex {
    name: String,
    expression: String,
    kind: IndexType,
    granularity: Option<u64>,
}

/// Supported ClickHouse table index type renderers.
#[derive(Debug, Clone)]
pub enum IndexType {
    VectorSimilarity(VectorSimilarityIndex),
    Custom(String),
}

/// Parameters for ClickHouse's `vector_similarity(...)` skipping index.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct VectorSimilarityIndex {
    algorithm: VectorIndexAlgorithm,
    distance: VectorDistanceFunction,
    dimensions: u64,
    quantization: Option<VectorQuantization>,
    hnsw_max_connections_per_layer: Option<u64>,
    hnsw_candidate_list_size_for_construction: Option<u64>,
}

/// Vector index implementation.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum VectorIndexAlgorithm {
    Hnsw,
}

/// Distance functions accepted by `vector_similarity(...)` indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum VectorDistanceFunction {
    L2Distance,
    CosineDistance,
}

/// Quantization values accepted by `vector_similarity(...)` indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum VectorQuantization {
    F64,
    F32,
    F16,
    BF16,
    I8,
    B1,
}

impl TableIndex {
    /// Build a `vector_similarity('hnsw', 'L2Distance', dimensions)` index.
    pub fn vector_similarity(
        name: impl Into<String>,
        expression: impl Into<String>,
        dimensions: u64,
    ) -> Self {
        Self {
            name: name.into(),
            expression: expression.into(),
            kind: IndexType::VectorSimilarity(VectorSimilarityIndex::new(dimensions)),
            granularity: None,
        }
    }

    /// Build a caller-provided index type expression.
    pub fn custom(
        name: impl Into<String>,
        expression: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            expression: expression.into(),
            kind: IndexType::Custom(kind.into()),
            granularity: None,
        }
    }

    /// Set the index granularity.
    pub fn granularity(mut self, granularity: u64) -> Self {
        self.granularity = Some(granularity);
        self
    }

    /// Set the vector distance function.
    pub fn distance(mut self, distance: VectorDistanceFunction) -> Self {
        if let IndexType::VectorSimilarity(index) = &mut self.kind {
            index.distance = distance;
        }
        self
    }

    /// Set the vector quantization.
    pub fn quantization(mut self, quantization: VectorQuantization) -> Self {
        if let IndexType::VectorSimilarity(index) = &mut self.kind {
            index.quantization = Some(quantization);
        }
        self
    }

    /// Set HNSW graph construction parameters.
    pub fn hnsw_params(
        mut self,
        max_connections_per_layer: u64,
        candidate_list_size_for_construction: u64,
    ) -> Self {
        if let IndexType::VectorSimilarity(index) = &mut self.kind {
            index.hnsw_max_connections_per_layer = Some(max_connections_per_layer);
            index.hnsw_candidate_list_size_for_construction =
                Some(candidate_list_size_for_construction);
        }
        self
    }
}

impl VectorSimilarityIndex {
    fn new(dimensions: u64) -> Self {
        Self {
            algorithm: VectorIndexAlgorithm::Hnsw,
            distance: VectorDistanceFunction::L2Distance,
            dimensions,
            quantization: None,
            hnsw_max_connections_per_layer: None,
            hnsw_candidate_list_size_for_construction: None,
        }
    }
}

/// One field in a ClickHouse `Nested(...)` DDL type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedField {
    name: String,
    data_type: DataType,
}

impl NestedField {
    /// Create a named field for `Nested(...)`.
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
        }
    }
}

/// ClickHouse data type syntax for DDL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataType {
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    Int128,
    Int256,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    UInt128,
    UInt256,
    Float32,
    Float64,
    BFloat16,
    Decimal32(u8),
    Decimal64(u8),
    Decimal128(u8),
    Decimal256(u8),
    Decimal {
        precision: u8,
        scale: u8,
    },
    String,
    Date,
    DateTime,
    DateTime64(u8),
    Uuid,
    Json,
    IPv4,
    IPv6,
    Enum8(Vec<(String, i8)>),
    Enum16(Vec<(String, i16)>),
    Array(Box<DataType>),
    Map(Box<DataType>, Box<DataType>),
    LowCardinality(Box<DataType>),
    Nullable(Box<DataType>),
    Tuple(Vec<DataType>),
    Nested(Vec<NestedField>),
    AggregateFunction {
        function: String,
        arguments: Vec<DataType>,
    },
    /// Caller-provided type expression, e.g. `"Decimal(18, 4)"`.
    Custom(String),
}

impl DataType {
    pub fn decimal32(scale: u8) -> Self {
        Self::Decimal32(scale)
    }

    pub fn decimal64(scale: u8) -> Self {
        Self::Decimal64(scale)
    }

    pub fn decimal128(scale: u8) -> Self {
        Self::Decimal128(scale)
    }

    pub fn decimal256(scale: u8) -> Self {
        Self::Decimal256(scale)
    }

    pub fn decimal(precision: u8, scale: u8) -> Self {
        Self::Decimal { precision, scale }
    }

    pub fn enum8<I, S>(variants: I) -> Self
    where
        I: IntoIterator<Item = (S, i8)>,
        S: Into<String>,
    {
        Self::Enum8(
            variants
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        )
    }

    pub fn enum16<I, S>(variants: I) -> Self
    where
        I: IntoIterator<Item = (S, i16)>,
        S: Into<String>,
    {
        Self::Enum16(
            variants
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        )
    }

    pub fn array(inner: DataType) -> Self {
        Self::Array(Box::new(inner))
    }

    pub fn map(key: DataType, value: DataType) -> Self {
        Self::Map(Box::new(key), Box::new(value))
    }

    pub fn low_cardinality(inner: DataType) -> Self {
        Self::LowCardinality(Box::new(inner))
    }

    pub fn nullable(inner: DataType) -> Self {
        Self::Nullable(Box::new(inner))
    }

    pub fn tuple<I>(types: I) -> Self
    where
        I: IntoIterator<Item = DataType>,
    {
        Self::Tuple(types.into_iter().collect())
    }

    pub fn nested<I>(fields: I) -> Self
    where
        I: IntoIterator<Item = NestedField>,
    {
        Self::Nested(fields.into_iter().collect())
    }

    pub fn aggregate_function<I>(function: impl Into<String>, arguments: I) -> Self
    where
        I: IntoIterator<Item = DataType>,
    {
        Self::AggregateFunction {
            function: function.into(),
            arguments: arguments.into_iter().collect(),
        }
    }

    pub fn custom(value: impl Into<String>) -> Self {
        Self::Custom(value.into())
    }
}

/// Supported table engine builders.
#[derive(Debug, Clone)]
pub enum TableEngine {
    Memory,
    Null,
    MergeTree(MergeTree),
    Distributed(DistributedEngine),
    Buffer(BufferEngine),
    /// Caller-provided engine expression, e.g. `"Distributed(cluster, db, table)"`.
    Custom(String),
}

impl TableEngine {
    pub fn memory() -> Self {
        Self::Memory
    }

    pub fn null() -> Self {
        Self::Null
    }

    pub fn custom(value: impl Into<String>) -> Self {
        Self::Custom(value.into())
    }
}

impl From<MergeTree> for TableEngine {
    fn from(value: MergeTree) -> Self {
        Self::MergeTree(value)
    }
}

impl From<DistributedEngine> for TableEngine {
    fn from(value: DistributedEngine) -> Self {
        Self::Distributed(value)
    }
}

impl From<BufferEngine> for TableEngine {
    fn from(value: BufferEngine) -> Self {
        Self::Buffer(value)
    }
}

/// `Distributed(cluster, database, table[, sharding_key[, policy_name]])` engine definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DistributedEngine {
    cluster: String,
    database: String,
    table: String,
    sharding_key: Option<String>,
    policy_name: Option<String>,
    settings: Vec<EngineSetting>,
}

impl DistributedEngine {
    fn new(
        cluster: impl Into<String>,
        database: impl Into<String>,
        table: impl Into<String>,
    ) -> Self {
        Self {
            cluster: cluster.into(),
            database: database.into(),
            table: table.into(),
            sharding_key: None,
            policy_name: None,
            settings: Vec::new(),
        }
    }

    /// Add the optional sharding-key expression.
    pub fn sharding_key(mut self, expr: impl Into<String>) -> Self {
        self.sharding_key = Some(expr.into());
        self
    }

    /// Add the optional storage policy name.
    pub fn policy_name(mut self, name: impl Into<String>) -> Self {
        self.policy_name = Some(name.into());
        self
    }

    /// Add one Distributed engine setting.
    pub fn setting(
        mut self,
        name: impl Into<String>,
        value: impl Into<EngineSettingValue>,
    ) -> Self {
        self.settings.push(EngineSetting {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

/// `Buffer(database, table, ...)` engine definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferEngine {
    database: String,
    table: String,
    num_layers: u64,
    min_time: u64,
    max_time: u64,
    min_rows: u64,
    max_rows: u64,
    min_bytes: u64,
    max_bytes: u64,
    flush_time: Option<u64>,
    flush_rows: Option<u64>,
    flush_bytes: Option<u64>,
}

impl BufferEngine {
    #[allow(clippy::too_many_arguments)]
    fn new(
        database: impl Into<String>,
        table: impl Into<String>,
        num_layers: u64,
        min_time: u64,
        max_time: u64,
        min_rows: u64,
        max_rows: u64,
        min_bytes: u64,
        max_bytes: u64,
    ) -> Self {
        Self {
            database: database.into(),
            table: table.into(),
            num_layers,
            min_time,
            max_time,
            min_rows,
            max_rows,
            min_bytes,
            max_bytes,
            flush_time: None,
            flush_rows: None,
            flush_bytes: None,
        }
    }

    /// Add the optional `flush_time` threshold.
    pub fn flush_time(mut self, value: u64) -> Self {
        self.flush_time = Some(value);
        self
    }

    /// Add the optional `flush_rows` threshold.
    pub fn flush_rows(mut self, value: u64) -> Self {
        self.flush_rows = Some(value);
        self
    }

    /// Add the optional `flush_bytes` threshold.
    pub fn flush_bytes(mut self, value: u64) -> Self {
        self.flush_bytes = Some(value);
        self
    }
}

/// MergeTree-family engine definition.
#[derive(Debug, Clone)]
pub struct MergeTree {
    kind: MergeTreeKind,
    partition_by: Option<Vec<String>>,
    primary_key: Option<Vec<String>>,
    order_by: Option<Vec<String>>,
    sample_by: Option<String>,
    ttl: Option<String>,
    settings: Vec<EngineSetting>,
}

#[derive(Debug, Clone)]
enum MergeTreeKind {
    Base,
    Replacing { version: Option<String> },
    Summing { columns: Option<Vec<String>> },
    Aggregating,
    Collapsing { sign: String },
    VersionedCollapsing { sign: String, version: String },
}

impl MergeTree {
    fn new(kind: MergeTreeKind) -> Self {
        Self {
            kind,
            partition_by: None,
            primary_key: None,
            order_by: None,
            sample_by: None,
            ttl: None,
            settings: Vec::new(),
        }
    }

    /// Add `PARTITION BY expr[, ...]`.
    pub fn partition_by<I, S>(mut self, exprs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.partition_by = Some(exprs.into_iter().map(Into::into).collect());
        self
    }

    /// Add `PRIMARY KEY expr[, ...]`.
    pub fn primary_key<I, S>(mut self, exprs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.primary_key = Some(exprs.into_iter().map(Into::into).collect());
        self
    }

    /// Add `ORDER BY expr[, ...]`.
    pub fn order_by<I, S>(mut self, exprs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.order_by = Some(exprs.into_iter().map(Into::into).collect());
        self
    }

    /// Add `SAMPLE BY expr`.
    pub fn sample_by(mut self, expr: impl Into<String>) -> Self {
        self.sample_by = Some(expr.into());
        self
    }

    /// Add `TTL expr`.
    pub fn ttl(mut self, expr: impl Into<String>) -> Self {
        self.ttl = Some(expr.into());
        self
    }

    /// Add one engine setting.
    pub fn setting(
        mut self,
        name: impl Into<String>,
        value: impl Into<EngineSettingValue>,
    ) -> Self {
        self.settings.push(EngineSetting {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

/// One `SETTINGS name = value` item in an engine definition.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineSetting {
    name: String,
    value: EngineSettingValue,
}

/// Literal value in a MergeTree `SETTINGS` clause.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineSettingValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
}

impl From<bool> for EngineSettingValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<i64> for EngineSettingValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
impl From<i32> for EngineSettingValue {
    fn from(value: i32) -> Self {
        Self::Int(value.into())
    }
}
impl From<u64> for EngineSettingValue {
    fn from(value: u64) -> Self {
        Self::UInt(value)
    }
}
impl From<u32> for EngineSettingValue {
    fn from(value: u32) -> Self {
        Self::UInt(value.into())
    }
}
impl From<f64> for EngineSettingValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}
impl From<f32> for EngineSettingValue {
    fn from(value: f32) -> Self {
        Self::Float(value.into())
    }
}
impl From<String> for EngineSettingValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<&str> for EngineSettingValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl QueryId for CreateTable {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl<Query> QueryId for CreateMaterializedView<Query>
where
    Query: QueryId,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl QueryId for AlterTable {
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl QueryFragment<ClickHouse> for CreateTable {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.columns.is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse CREATE TABLE requires at least one column".into(),
            ));
        }

        out.push_sql("CREATE TABLE ");
        if self.if_not_exists {
            out.push_sql("IF NOT EXISTS ");
        }
        push_qualified_identifier(&mut out, &self.name)?;
        out.push_sql(" (\n");
        for (idx, column) in self.columns.iter().enumerate() {
            if idx > 0 {
                out.push_sql(",\n");
            }
            out.push_sql("    ");
            column.walk_ast(out.reborrow())?;
        }
        for index in &self.indexes {
            out.push_sql(",\n    ");
            index.walk_ast(out.reborrow())?;
        }
        for projection in &self.projections {
            out.push_sql(",\n    ");
            projection.walk_ast(out.reborrow())?;
        }
        out.push_sql("\n)");
        if let Some(engine) = &self.engine {
            out.push_sql(" ENGINE = ");
            engine.walk_ast(out.reborrow())?;
        }
        Ok(())
    }
}

impl<Query> QueryFragment<ClickHouse> for CreateMaterializedView<Query>
where
    Query: QueryFragment<ClickHouse>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        match (&self.target, &self.engine) {
            (Some(_), Some(_)) => {
                return Err(Error::QueryBuilderError(
                    "ClickHouse materialized view cannot use both TO and ENGINE".into(),
                ));
            }
            (None, None) => {
                return Err(Error::QueryBuilderError(
                    "ClickHouse materialized view requires TO target or ENGINE".into(),
                ));
            }
            _ => {}
        }

        out.push_sql("CREATE MATERIALIZED VIEW ");
        if self.if_not_exists {
            out.push_sql("IF NOT EXISTS ");
        }
        push_qualified_identifier(&mut out, &self.name)?;
        if let Some(target) = &self.target {
            out.push_sql(" TO ");
            push_qualified_identifier(&mut out, target)?;
        }
        if let Some(engine) = &self.engine {
            out.push_sql(" ENGINE = ");
            engine.walk_ast(out.reborrow())?;
        }
        if self.populate {
            out.push_sql(" POPULATE");
        }
        out.push_sql(" AS ");
        self.query.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for AlterTable {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        let Some(operation) = &self.operation else {
            return Err(Error::QueryBuilderError(
                "ClickHouse ALTER TABLE requires an operation".into(),
            ));
        };

        out.push_sql("ALTER TABLE ");
        push_qualified_identifier(&mut out, &self.name)?;
        out.push_sql(" ");
        operation.walk_ast(out.reborrow())?;
        if !self.settings.is_empty() {
            out.push_sql(" SETTINGS ");
            for (idx, setting) in self.settings.iter().enumerate() {
                if idx > 0 {
                    out.push_sql(", ");
                }
                validate_bare_identifier(&setting.name, "setting")?;
                out.push_sql(&setting.name);
                out.push_sql(" = ");
                push_setting_value(&mut out, &setting.value)?;
            }
        }
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for AlterTableOperation {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        match self {
            Self::AddColumn { column, after } => {
                out.push_sql("ADD COLUMN ");
                column.walk_ast(out.reborrow())?;
                if let Some(after) = after {
                    out.push_sql(" AFTER ");
                    push_qualified_identifier(&mut out, after)?;
                }
            }
            Self::DropColumn { name } => {
                out.push_sql("DROP COLUMN ");
                push_qualified_identifier(&mut out, name)?;
            }
            Self::RenameColumn { from, to } => {
                out.push_sql("RENAME COLUMN ");
                push_qualified_identifier(&mut out, from)?;
                out.push_sql(" TO ");
                push_qualified_identifier(&mut out, to)?;
            }
            Self::AddIndex(index) => {
                out.push_sql("ADD ");
                index.walk_ast(out.reborrow())?;
            }
            Self::DropIndex { name } => {
                out.push_sql("DROP INDEX ");
                validate_bare_identifier(name, "index")?;
                out.push_identifier(name)?;
            }
            Self::MaterializeIndex { name } => {
                out.push_sql("MATERIALIZE INDEX ");
                validate_bare_identifier(name, "index")?;
                out.push_identifier(name)?;
            }
            Self::ModifyTtl { expr } => {
                out.push_sql("MODIFY TTL ");
                out.push_sql(expr);
            }
            Self::AddProjection(projection) => {
                out.push_sql("ADD ");
                projection.walk_ast(out.reborrow())?;
            }
            Self::DropProjection { name } => {
                out.push_sql("DROP PROJECTION ");
                validate_bare_identifier(name, "projection")?;
                out.push_identifier(name)?;
            }
            Self::MaterializeProjection { name } => {
                out.push_sql("MATERIALIZE PROJECTION ");
                validate_bare_identifier(name, "projection")?;
                out.push_identifier(name)?;
            }
        }
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for Column {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        push_qualified_identifier(&mut out, &self.name)?;
        out.push_sql(" ");
        self.data_type.walk_ast(out.reborrow())?;
        if let Some(default) = &self.default {
            match default {
                ColumnDefault::Default(expr) => {
                    out.push_sql(" DEFAULT ");
                    out.push_sql(expr);
                }
                ColumnDefault::Materialized(expr) => {
                    out.push_sql(" MATERIALIZED ");
                    out.push_sql(expr);
                }
                ColumnDefault::Alias(expr) => {
                    out.push_sql(" ALIAS ");
                    out.push_sql(expr);
                }
            }
        }
        if let Some(codec) = &self.codec {
            out.push_sql(" CODEC(");
            out.push_sql(codec);
            out.push_sql(")");
        }
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for TableProjection {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.body.trim().is_empty() {
            return Err(Error::QueryBuilderError(
                "ClickHouse projection body must not be empty".into(),
            ));
        }
        out.push_sql("PROJECTION ");
        validate_bare_identifier(&self.name, "projection")?;
        out.push_identifier(&self.name)?;
        out.push_sql(" (");
        out.push_sql(&self.body);
        out.push_sql(")");
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for TableIndex {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.granularity == Some(0) {
            return Err(Error::QueryBuilderError(
                "ClickHouse index granularity must be greater than 0".into(),
            ));
        }
        out.push_sql("INDEX ");
        validate_bare_identifier(&self.name, "index")?;
        out.push_identifier(&self.name)?;
        out.push_sql(" ");
        out.push_sql(&self.expression);
        out.push_sql(" TYPE ");
        self.kind.walk_ast(out.reborrow())?;
        if let Some(granularity) = self.granularity {
            out.push_sql(" GRANULARITY ");
            out.push_sql(&granularity.to_string());
        }
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for IndexType {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        match self {
            Self::VectorSimilarity(index) => index.walk_ast(out.reborrow())?,
            Self::Custom(value) => out.push_sql(value),
        }
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for VectorSimilarityIndex {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        if self.dimensions == 0 {
            return Err(Error::QueryBuilderError(
                "ClickHouse vector similarity index dimensions must be greater than 0".into(),
            ));
        }
        let has_hnsw_params = self.hnsw_max_connections_per_layer.is_some()
            || self.hnsw_candidate_list_size_for_construction.is_some();
        if has_hnsw_params
            && (self.hnsw_max_connections_per_layer.is_none()
                || self.hnsw_candidate_list_size_for_construction.is_none())
        {
            return Err(Error::QueryBuilderError(
                "ClickHouse vector similarity HNSW parameters must be set together".into(),
            ));
        }

        out.push_sql("vector_similarity(");
        push_string_literal(&mut out, self.algorithm.as_sql());
        out.push_sql(", ");
        push_string_literal(&mut out, self.distance.as_sql());
        out.push_sql(", ");
        out.push_sql(&self.dimensions.to_string());
        if let Some(quantization) = self.quantization {
            out.push_sql(", ");
            push_string_literal(&mut out, quantization.as_sql());
        }
        if let (Some(max_connections), Some(candidate_size)) = (
            self.hnsw_max_connections_per_layer,
            self.hnsw_candidate_list_size_for_construction,
        ) {
            if self.quantization.is_none() {
                out.push_sql(", ");
                push_string_literal(&mut out, VectorQuantization::BF16.as_sql());
            }
            out.push_sql(", ");
            out.push_sql(&max_connections.to_string());
            out.push_sql(", ");
            out.push_sql(&candidate_size.to_string());
        }
        out.push_sql(")");
        Ok(())
    }
}

impl VectorIndexAlgorithm {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Hnsw => "hnsw",
        }
    }
}

impl VectorDistanceFunction {
    fn as_sql(self) -> &'static str {
        match self {
            Self::L2Distance => "L2Distance",
            Self::CosineDistance => "cosineDistance",
        }
    }
}

impl VectorQuantization {
    fn as_sql(self) -> &'static str {
        match self {
            Self::F64 => "f64",
            Self::F32 => "f32",
            Self::F16 => "f16",
            Self::BF16 => "bf16",
            Self::I8 => "i8",
            Self::B1 => "b1",
        }
    }
}

impl QueryFragment<ClickHouse> for DataType {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        match self {
            Self::Bool => out.push_sql("Bool"),
            Self::Int8 => out.push_sql("Int8"),
            Self::Int16 => out.push_sql("Int16"),
            Self::Int32 => out.push_sql("Int32"),
            Self::Int64 => out.push_sql("Int64"),
            Self::Int128 => out.push_sql("Int128"),
            Self::Int256 => out.push_sql("Int256"),
            Self::UInt8 => out.push_sql("UInt8"),
            Self::UInt16 => out.push_sql("UInt16"),
            Self::UInt32 => out.push_sql("UInt32"),
            Self::UInt64 => out.push_sql("UInt64"),
            Self::UInt128 => out.push_sql("UInt128"),
            Self::UInt256 => out.push_sql("UInt256"),
            Self::Float32 => out.push_sql("Float32"),
            Self::Float64 => out.push_sql("Float64"),
            Self::BFloat16 => out.push_sql("BFloat16"),
            Self::Decimal32(scale) => push_decimal_family(&mut out, "Decimal32", *scale, 9)?,
            Self::Decimal64(scale) => push_decimal_family(&mut out, "Decimal64", *scale, 18)?,
            Self::Decimal128(scale) => push_decimal_family(&mut out, "Decimal128", *scale, 38)?,
            Self::Decimal256(scale) => push_decimal_family(&mut out, "Decimal256", *scale, 76)?,
            Self::Decimal { precision, scale } => {
                push_decimal(&mut out, *precision, *scale)?;
            }
            Self::String => out.push_sql("String"),
            Self::Date => out.push_sql("Date"),
            Self::DateTime => out.push_sql("DateTime"),
            Self::DateTime64(scale) => {
                out.push_sql("DateTime64(");
                out.push_sql(&scale.to_string());
                out.push_sql(")");
            }
            Self::Uuid => out.push_sql("UUID"),
            Self::Json => out.push_sql("JSON"),
            Self::IPv4 => out.push_sql("IPv4"),
            Self::IPv6 => out.push_sql("IPv6"),
            Self::Enum8(variants) => push_enum_variants(&mut out, "Enum8", variants)?,
            Self::Enum16(variants) => push_enum_variants(&mut out, "Enum16", variants)?,
            Self::Array(inner) => {
                out.push_sql("Array(");
                inner.walk_ast(out.reborrow())?;
                out.push_sql(")");
            }
            Self::Map(key, value) => {
                out.push_sql("Map(");
                key.walk_ast(out.reborrow())?;
                out.push_sql(", ");
                value.walk_ast(out.reborrow())?;
                out.push_sql(")");
            }
            Self::LowCardinality(inner) => {
                out.push_sql("LowCardinality(");
                inner.walk_ast(out.reborrow())?;
                out.push_sql(")");
            }
            Self::Nullable(inner) => {
                out.push_sql("Nullable(");
                inner.walk_ast(out.reborrow())?;
                out.push_sql(")");
            }
            Self::Tuple(types) => push_tuple_type(&mut out, types)?,
            Self::Nested(fields) => push_nested_type(&mut out, fields)?,
            Self::AggregateFunction {
                function,
                arguments,
            } => {
                validate_bare_identifier(function, "aggregate function")?;
                out.push_sql("AggregateFunction(");
                out.push_sql(function);
                for argument in arguments {
                    out.push_sql(", ");
                    argument.walk_ast(out.reborrow())?;
                }
                out.push_sql(")");
            }
            Self::Custom(value) => out.push_sql(value),
        }
        Ok(())
    }
}

fn push_decimal_family(
    out: &mut AstPass<'_, '_, ClickHouse>,
    family: &'static str,
    scale: u8,
    max_scale: u8,
) -> QueryResult<()> {
    if scale > max_scale {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse {family} scale must be <= {max_scale}, got {scale}").into(),
        ));
    }
    out.push_sql(family);
    out.push_sql("(");
    out.push_sql(&scale.to_string());
    out.push_sql(")");
    Ok(())
}

fn push_decimal(
    out: &mut AstPass<'_, '_, ClickHouse>,
    precision: u8,
    scale: u8,
) -> QueryResult<()> {
    if precision == 0 || precision > 76 {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse Decimal precision must be between 1 and 76, got {precision}")
                .into(),
        ));
    }
    if scale > precision {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse Decimal scale must be <= precision, got {scale} > {precision}")
                .into(),
        ));
    }
    out.push_sql("Decimal(");
    out.push_sql(&precision.to_string());
    out.push_sql(", ");
    out.push_sql(&scale.to_string());
    out.push_sql(")");
    Ok(())
}

fn push_enum_variants<T>(
    out: &mut AstPass<'_, '_, ClickHouse>,
    family: &'static str,
    variants: &[(String, T)],
) -> QueryResult<()>
where
    T: std::fmt::Display,
{
    if variants.is_empty() {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse {family} requires at least one variant").into(),
        ));
    }
    out.push_sql(family);
    out.push_sql("(");
    for (idx, (name, value)) in variants.iter().enumerate() {
        if idx > 0 {
            out.push_sql(", ");
        }
        push_string_literal(out, name);
        out.push_sql(" = ");
        out.push_sql(&value.to_string());
    }
    out.push_sql(")");
    Ok(())
}

fn push_tuple_type<'b>(
    out: &mut AstPass<'_, 'b, ClickHouse>,
    types: &'b [DataType],
) -> QueryResult<()> {
    if types.is_empty() {
        return Err(Error::QueryBuilderError(
            "ClickHouse Tuple requires at least one element".into(),
        ));
    }
    out.push_sql("Tuple(");
    for (idx, data_type) in types.iter().enumerate() {
        if idx > 0 {
            out.push_sql(", ");
        }
        data_type.walk_ast(out.reborrow())?;
    }
    out.push_sql(")");
    Ok(())
}

fn push_nested_type<'b>(
    out: &mut AstPass<'_, 'b, ClickHouse>,
    fields: &'b [NestedField],
) -> QueryResult<()> {
    if fields.is_empty() {
        return Err(Error::QueryBuilderError(
            "ClickHouse Nested requires at least one field".into(),
        ));
    }
    out.push_sql("Nested(");
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            out.push_sql(", ");
        }
        validate_bare_identifier(&field.name, "nested field")?;
        out.push_identifier(&field.name)?;
        out.push_sql(" ");
        field.data_type.walk_ast(out.reborrow())?;
    }
    out.push_sql(")");
    Ok(())
}

impl QueryFragment<ClickHouse> for TableEngine {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        match self {
            Self::Memory => out.push_sql("Memory"),
            Self::Null => out.push_sql("Null"),
            Self::MergeTree(engine) => engine.walk_ast(out.reborrow())?,
            Self::Distributed(engine) => engine.walk_ast(out.reborrow())?,
            Self::Buffer(engine) => engine.walk_ast(out.reborrow())?,
            Self::Custom(engine) => out.push_sql(engine),
        }
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for DistributedEngine {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        validate_non_empty(&self.cluster, "Distributed cluster")?;
        validate_non_empty(&self.database, "Distributed database")?;
        validate_non_empty(&self.table, "Distributed table")?;
        if self.policy_name.is_some() && self.sharding_key.is_none() {
            return Err(Error::QueryBuilderError(
                "ClickHouse Distributed policy_name requires a sharding_key argument".into(),
            ));
        }

        out.push_sql("Distributed(");
        out.push_sql(&self.cluster);
        out.push_sql(", ");
        out.push_sql(&self.database);
        out.push_sql(", ");
        out.push_sql(&self.table);
        if let Some(sharding_key) = &self.sharding_key {
            validate_non_empty(sharding_key, "Distributed sharding key")?;
            out.push_sql(", ");
            out.push_sql(sharding_key);
        }
        if let Some(policy_name) = &self.policy_name {
            validate_non_empty(policy_name, "Distributed policy name")?;
            out.push_sql(", ");
            out.push_sql(policy_name);
        }
        out.push_sql(")");
        push_engine_settings(&mut out, &self.settings)?;
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for BufferEngine {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        validate_non_empty(&self.database, "Buffer database")?;
        validate_non_empty(&self.table, "Buffer table")?;
        if self.num_layers == 0 {
            return Err(Error::QueryBuilderError(
                "ClickHouse Buffer num_layers must be greater than 0".into(),
            ));
        }

        out.push_sql("Buffer(");
        out.push_sql(&self.database);
        out.push_sql(", ");
        out.push_sql(&self.table);
        for value in [
            self.num_layers,
            self.min_time,
            self.max_time,
            self.min_rows,
            self.max_rows,
            self.min_bytes,
            self.max_bytes,
        ] {
            out.push_sql(", ");
            out.push_sql(&value.to_string());
        }
        if let Some(flush_time) = self.flush_time {
            out.push_sql(", ");
            out.push_sql(&flush_time.to_string());
        }
        if let Some(flush_rows) = self.flush_rows {
            if self.flush_time.is_none() {
                return Err(Error::QueryBuilderError(
                    "ClickHouse Buffer flush_rows requires flush_time".into(),
                ));
            }
            out.push_sql(", ");
            out.push_sql(&flush_rows.to_string());
        }
        if let Some(flush_bytes) = self.flush_bytes {
            if self.flush_time.is_none() || self.flush_rows.is_none() {
                return Err(Error::QueryBuilderError(
                    "ClickHouse Buffer flush_bytes requires flush_time and flush_rows".into(),
                ));
            }
            out.push_sql(", ");
            out.push_sql(&flush_bytes.to_string());
        }
        out.push_sql(")");
        Ok(())
    }
}

impl QueryFragment<ClickHouse> for MergeTree {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, ClickHouse>) -> QueryResult<()> {
        match &self.kind {
            MergeTreeKind::Base => out.push_sql("MergeTree"),
            MergeTreeKind::Replacing { version: None } => out.push_sql("ReplacingMergeTree"),
            MergeTreeKind::Replacing {
                version: Some(version),
            } => {
                validate_bare_identifier(version, "ReplacingMergeTree version column")?;
                out.push_sql("ReplacingMergeTree(");
                out.push_sql(version);
                out.push_sql(")");
            }
            MergeTreeKind::Summing { columns: None } => out.push_sql("SummingMergeTree"),
            MergeTreeKind::Summing {
                columns: Some(columns),
            } => {
                out.push_sql("SummingMergeTree(");
                push_engine_identifier_tuple(&mut out, columns)?;
                out.push_sql(")");
            }
            MergeTreeKind::Aggregating => out.push_sql("AggregatingMergeTree"),
            MergeTreeKind::Collapsing { sign } => {
                validate_bare_identifier(sign, "CollapsingMergeTree sign column")?;
                out.push_sql("CollapsingMergeTree(");
                out.push_sql(sign);
                out.push_sql(")");
            }
            MergeTreeKind::VersionedCollapsing { sign, version } => {
                validate_bare_identifier(sign, "VersionedCollapsingMergeTree sign column")?;
                validate_bare_identifier(version, "VersionedCollapsingMergeTree version column")?;
                out.push_sql("VersionedCollapsingMergeTree(");
                out.push_sql(sign);
                out.push_sql(", ");
                out.push_sql(version);
                out.push_sql(")");
            }
        }
        push_optional_expr_list(&mut out, " PARTITION BY ", self.partition_by.as_deref())?;
        push_optional_expr_list(&mut out, " PRIMARY KEY ", self.primary_key.as_deref())?;
        push_optional_expr_list(&mut out, " ORDER BY ", self.order_by.as_deref())?;
        if let Some(sample_by) = &self.sample_by {
            out.push_sql(" SAMPLE BY ");
            out.push_sql(sample_by);
        }
        if let Some(ttl) = &self.ttl {
            out.push_sql(" TTL ");
            out.push_sql(ttl);
        }
        push_engine_settings(&mut out, &self.settings)?;
        Ok(())
    }
}

fn push_engine_identifier_tuple(
    out: &mut AstPass<'_, '_, ClickHouse>,
    identifiers: &[String],
) -> QueryResult<()> {
    if identifiers.is_empty() {
        return Err(Error::QueryBuilderError(
            "ClickHouse engine column tuple requires at least one column".into(),
        ));
    }
    out.push_sql("(");
    for (idx, identifier) in identifiers.iter().enumerate() {
        if idx > 0 {
            out.push_sql(", ");
        }
        validate_bare_identifier(identifier, "engine column")?;
        out.push_sql(identifier);
    }
    out.push_sql(")");
    Ok(())
}

fn push_engine_settings(
    out: &mut AstPass<'_, '_, ClickHouse>,
    settings: &[EngineSetting],
) -> QueryResult<()> {
    if settings.is_empty() {
        return Ok(());
    }
    out.push_sql(" SETTINGS ");
    for (idx, setting) in settings.iter().enumerate() {
        if idx > 0 {
            out.push_sql(", ");
        }
        validate_bare_identifier(&setting.name, "setting")?;
        out.push_sql(&setting.name);
        out.push_sql(" = ");
        push_setting_value(out, &setting.value)?;
    }
    Ok(())
}

fn push_optional_expr_list(
    out: &mut AstPass<'_, '_, ClickHouse>,
    prefix: &str,
    exprs: Option<&[String]>,
) -> QueryResult<()> {
    let Some(exprs) = exprs else {
        return Ok(());
    };
    if exprs.is_empty() {
        return Err(Error::QueryBuilderError(
            format!("ClickHouse clause {prefix:?} requires at least one expression").into(),
        ));
    }
    out.push_sql(prefix);
    if exprs.len() == 1 {
        out.push_sql(&exprs[0]);
    } else {
        out.push_sql("(");
        for (idx, expr) in exprs.iter().enumerate() {
            if idx > 0 {
                out.push_sql(", ");
            }
            out.push_sql(expr);
        }
        out.push_sql(")");
    }
    Ok(())
}

fn push_setting_value(
    out: &mut AstPass<'_, '_, ClickHouse>,
    value: &EngineSettingValue,
) -> QueryResult<()> {
    match value {
        EngineSettingValue::Bool(value) => out.push_sql(if *value { "1" } else { "0" }),
        EngineSettingValue::Int(value) => out.push_sql(&value.to_string()),
        EngineSettingValue::UInt(value) => out.push_sql(&value.to_string()),
        EngineSettingValue::Float(value) => {
            if !value.is_finite() {
                return Err(Error::QueryBuilderError(
                    format!("ClickHouse setting value must be finite, got {value}").into(),
                ));
            }
            out.push_sql(&value.to_string());
        }
        EngineSettingValue::String(value) => push_string_literal(out, value),
    }
    Ok(())
}

fn push_string_literal(out: &mut AstPass<'_, '_, ClickHouse>, value: &str) {
    out.push_sql("'");
    let mut remaining = value;
    while let Some(idx) = remaining.find('\'') {
        out.push_sql(&remaining[..idx]);
        out.push_sql("''");
        remaining = &remaining[idx + 1..];
    }
    out.push_sql(remaining);
    out.push_sql("'");
}

fn push_qualified_identifier(
    out: &mut AstPass<'_, '_, ClickHouse>,
    value: &str,
) -> QueryResult<()> {
    if value.trim().is_empty() {
        return Err(Error::QueryBuilderError(
            "empty ClickHouse identifier".into(),
        ));
    }

    for (idx, part) in value.split('.').enumerate() {
        validate_bare_identifier(part, "identifier")?;
        if idx > 0 {
            out.push_sql(".");
        }
        out.push_identifier(part)?;
    }
    Ok(())
}

fn validate_non_empty(value: &str, kind: &str) -> QueryResult<()> {
    if value.trim().is_empty() {
        return Err(Error::QueryBuilderError(
            format!("empty ClickHouse {kind}").into(),
        ));
    }
    Ok(())
}

fn validate_bare_identifier(value: &str, kind: &str) -> QueryResult<()> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(Error::QueryBuilderError(
            format!("empty ClickHouse {kind}").into(),
        ));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse {kind}: {value:?}").into(),
        ));
    }
    if chars.any(|ch| !(ch == '_' || ch.is_ascii_alphanumeric())) {
        return Err(Error::QueryBuilderError(
            format!("invalid ClickHouse {kind}: {value:?}").into(),
        ));
    }
    Ok(())
}
