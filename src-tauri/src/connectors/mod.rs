pub mod bigquery;
pub mod clickhouse;
pub mod databricks;
pub mod files;
pub mod jwt;
pub mod motherduck;
pub mod mssql;
pub mod remote;
pub mod snapshot;
pub mod snowflake;
pub mod sql;
pub mod watcher;

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::duck::Analytics;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorKind {
    File,
    Folder,
    Postgres,
    Mysql,
    Sqlite,
    Supabase,
    Redshift,
    Motherduck,
    Sqlserver,
    Clickhouse,
    Snowflake,
    Bigquery,
    Databricks,
}

/// Whether a connector's tables are read in place (live DuckDB view) or copied once as a read-only snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    Live,
    Snapshot,
}

impl ConnectorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Sqlite => "sqlite",
            Self::Supabase => "supabase",
            Self::Redshift => "redshift",
            Self::Motherduck => "motherduck",
            Self::Sqlserver => "sqlserver",
            Self::Clickhouse => "clickhouse",
            Self::Snowflake => "snowflake",
            Self::Bigquery => "bigquery",
            Self::Databricks => "databricks",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ConnectorError> {
        let all = [
            Self::File,
            Self::Folder,
            Self::Postgres,
            Self::Mysql,
            Self::Sqlite,
            Self::Supabase,
            Self::Redshift,
            Self::Motherduck,
            Self::Sqlserver,
            Self::Clickhouse,
            Self::Snowflake,
            Self::Bigquery,
            Self::Databricks,
        ];
        all.into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| ConnectorError::Unsupported(format!("Unknown connector kind: {value}")))
    }

    pub fn is_path_based(self) -> bool {
        matches!(self, Self::File | Self::Folder)
    }

    pub fn access_mode(self) -> AccessMode {
        match self {
            Self::Postgres | Self::Mysql | Self::Sqlite | Self::Supabase | Self::Motherduck | Self::File | Self::Folder => {
                AccessMode::Live
            }
            _ => AccessMode::Snapshot,
        }
    }

    /// True when using this connector sends traffic off the device, so Offline mode must block it.
    pub fn leaves_device(self, host: &str) -> bool {
        match self {
            Self::File | Self::Folder | Self::Sqlite => false,
            Self::Postgres | Self::Mysql | Self::Sqlserver | Self::Clickhouse | Self::Redshift | Self::Supabase => {
                !matches!(host.trim().to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
            }
            _ => true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Failed(String),
}

impl From<duckdb::Error> for ConnectorError {
    fn from(error: duckdb::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

/// Where a discovered table physically lives, so registration never re-parses `source_ref`.
#[derive(Clone, Debug)]
pub enum Locator {
    File { path: PathBuf, format: files::FileFormat },
    Sheet { path: PathBuf, sheet: String },
}

#[derive(Clone, Debug)]
pub struct SourceTable {
    /// Stable identity of the table within its connection; survives re-discovery.
    pub source_ref: String,
    pub suggested_name: String,
    pub locator: Locator,
}

/// Default cap on rows copied by a snapshot import.
pub const DEFAULT_ROW_CAP: usize = 1_000_000;

#[derive(Clone, Serialize)]
pub struct ExploredColumn {
    pub name: String,
    pub data_type: String,
}

#[derive(Clone, Serialize)]
pub struct ExploredTable {
    pub name: String,
    /// -1 when the count was not fetched.
    pub row_count: i64,
    pub columns: Vec<ExploredColumn>,
    pub samples: Vec<BTreeMap<String, Value>>,
}

/// An all-text result set: used for read-only snapshot imports and ad-hoc read-only queries.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// True when the source had more rows than the cap and the rest were not copied.
    pub truncated: bool,
}

/// A data source the user configured. Implementations must only ever open read-only sessions.
#[async_trait]
pub trait Connector: Send + Sync {
    fn kind(&self) -> ConnectorKind;

    fn mode(&self) -> AccessMode {
        self.kind().access_mode()
    }

    async fn test(&self) -> Result<String, ConnectorError>;

    /// Names usable as `source_ref`, e.g. `schema.table`.
    async fn list_tables(&self) -> Result<Vec<String>, ConnectorError>;

    /// Columns, row count and a few sample rows for one table.
    async fn describe(&self, table: &str) -> Result<ExploredTable, ConnectorError>;

    /// Tables with columns and samples, for the Explore dialog.
    async fn explore(&self) -> Result<Vec<ExploredTable>, ConnectorError> {
        const DESCRIBE_LIMIT: usize = 100;
        let names = self.list_tables().await?;
        let mut tables = Vec::with_capacity(names.len());
        for (index, name) in names.into_iter().enumerate() {
            if index < DESCRIBE_LIMIT {
                tables.push(self.describe(&name).await?);
            } else {
                tables.push(ExploredTable { name, row_count: -1, columns: Vec::new(), samples: Vec::new() });
            }
        }
        Ok(tables)
    }

    /// Copies up to `row_cap` rows of a table as text. Snapshot connectors only.
    async fn fetch(&self, _table: &str, _row_cap: usize) -> Result<Snapshot, ConnectorError> {
        Err(ConnectorError::Unsupported("This connector reads tables live; there is nothing to snapshot".into()))
    }

    /// Runs one read-only SELECT against the source. Rejects anything else.
    #[allow(dead_code)] // read-only ad-hoc query hook for the analyst
    async fn query(&self, _sql: &str, _row_cap: usize) -> Result<Snapshot, ConnectorError> {
        Err(ConnectorError::Unsupported("Direct queries are not supported by this connector".into()))
    }

    /// Registers a live read-only DuckDB view. Live connectors only.
    fn scan(&self, _duck: &Analytics, _table: &str, _relation: &str) -> Result<(), ConnectorError> {
        Err(ConnectorError::Unsupported("This connector imports snapshots instead of live views".into()))
    }
}

/// Builds the connector for a saved or in-progress configuration.
pub fn build(config: sql::DbConfig) -> Result<Box<dyn Connector>, ConnectorError> {
    let kind = config.kind;
    let config = config.normalized()?;
    match kind {
        ConnectorKind::Postgres | ConnectorKind::Mysql | ConnectorKind::Sqlite | ConnectorKind::Supabase | ConnectorKind::Redshift => {
            Ok(Box::new(sql::SqlConnector::new(config)?))
        }
        ConnectorKind::Motherduck => Ok(Box::new(motherduck::MotherDuck::new(config))),
        ConnectorKind::Sqlserver => Ok(remote::RemoteConnector::boxed(config.clone(), Box::new(mssql::SqlServer::new(&config)))),
        ConnectorKind::Clickhouse => Ok(remote::RemoteConnector::boxed(config.clone(), Box::new(clickhouse::ClickHouse::new(&config)?))),
        ConnectorKind::Snowflake => Ok(remote::RemoteConnector::boxed(config.clone(), Box::new(snowflake::Snowflake::new(&config)?))),
        ConnectorKind::Bigquery => Ok(remote::RemoteConnector::boxed(config.clone(), Box::new(bigquery::BigQuery::new(&config)?))),
        ConnectorKind::Databricks => Ok(remote::RemoteConnector::boxed(config.clone(), Box::new(databricks::Databricks::new(&config)?))),
        _ => Err(ConnectorError::Unsupported("Not a database connector kind".into())),
    }
}

/// Secrets must never appear in errors that reach the UI or the audit log.
pub(crate) fn redact(error: impl std::fmt::Display, secret: &str) -> String {
    let message = error.to_string();
    if secret.is_empty() {
        message
    } else {
        message.replace(secret, "********")
    }
}

/// A source whose tables are exposed to the analytics engine as DuckDB relations.
pub trait DuckDbSource: Send + Sync {
    fn discover(&self) -> Result<Vec<SourceTable>, ConnectorError>;
    fn register(&self, duck: &Analytics, table: &SourceTable, relation: &str) -> Result<(), ConnectorError>;
}
