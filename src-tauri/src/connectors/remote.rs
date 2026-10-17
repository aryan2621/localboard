//! Shared plumbing for snapshot connectors that speak SQL over an API or wire protocol:
//! each backend only runs one statement; discovery, describe, capped fetch and the read-only guard live here.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;

use super::sql::DbConfig;
use super::{Connector, ConnectorError, ConnectorKind, ExploredColumn, ExploredTable, Snapshot};

#[async_trait]
pub trait Backend: Send + Sync {
    /// Runs one statement and returns at most `max_rows` rows (all values as text).
    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, String>;
}

pub fn cell(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

/// Only a single SELECT / WITH statement may be sent to a remote warehouse.
pub fn ensure_read_only(sql: &str) -> Result<(), String> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lowered = trimmed.to_ascii_lowercase();
    let starts = lowered.starts_with("select") || lowered.starts_with("with");
    if !starts || trimmed.contains(';') {
        return Err("Only a single read-only SELECT statement is allowed".into());
    }
    Ok(())
}

pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub struct RemoteConnector {
    kind: ConnectorKind,
    config: DbConfig,
    backend: Box<dyn Backend>,
}

impl RemoteConnector {
    pub fn boxed(config: DbConfig, backend: Box<dyn Backend>) -> Box<dyn Connector> {
        Box::new(Self { kind: config.kind, config, backend })
    }

    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, ConnectorError> {
        ensure_read_only(sql).map_err(ConnectorError::Unsupported)?;
        self.backend.run(sql, max_rows).await.map_err(|error| ConnectorError::Failed(super::redact(error, &self.config.password)))
    }

    fn quote(&self, name: &str) -> String {
        match self.kind {
            ConnectorKind::Sqlserver => format!("[{}]", name.replace(']', "]]")),
            ConnectorKind::Clickhouse | ConnectorKind::Bigquery | ConnectorKind::Databricks => format!("`{}`", name.replace('`', "\\`")),
            _ => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }

    fn split<'a>(&self, table: &'a str) -> (Option<&'a str>, &'a str) {
        match (self.kind, table.split_once('.')) {
            (ConnectorKind::Bigquery | ConnectorKind::Databricks | ConnectorKind::Clickhouse, _) => (None, table),
            (_, Some((schema, name))) => (Some(schema), name),
            (ConnectorKind::Sqlserver, None) => (Some("dbo"), table),
            (_, None) => (None, table),
        }
    }

    fn dataset(&self) -> &str {
        self.config.extra("dataset")
    }

    fn db_schema(&self) -> &str {
        let schema = self.config.extra("schema");
        if schema.is_empty() { "default" } else { schema }
    }

    fn qualified(&self, table: &str) -> String {
        let (schema, name) = self.split(table);
        match self.kind {
            ConnectorKind::Bigquery => {
                format!("`{}.{}.{}`", self.config.database, self.dataset(), name).replace("..", ".")
            }
            ConnectorKind::Clickhouse => format!("{}.{}", self.quote(&self.config.database), self.quote(name)),
            ConnectorKind::Databricks => {
                format!("{}.{}.{}", self.quote(&self.config.database), self.quote(self.db_schema()), self.quote(name))
            }
            _ => match schema {
                Some(schema) => format!("{}.{}", self.quote(schema), self.quote(name)),
                None => self.quote(name),
            },
        }
    }

    fn list_sql(&self) -> String {
        match self.kind {
            ConnectorKind::Sqlserver => "SELECT CAST(TABLE_SCHEMA + '.' + TABLE_NAME AS NVARCHAR(512)) FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE' ORDER BY 1".into(),
            ConnectorKind::Clickhouse => format!(
                "SELECT name FROM system.tables WHERE database = {} AND NOT is_temporary ORDER BY name",
                literal(&self.config.database)
            ),
            ConnectorKind::Snowflake => "SELECT table_schema || '.' || table_name FROM information_schema.tables WHERE table_type = 'BASE TABLE' AND table_schema <> 'INFORMATION_SCHEMA' ORDER BY 1".into(),
            ConnectorKind::Bigquery => format!(
                "SELECT table_name FROM `{}.{}`.INFORMATION_SCHEMA.TABLES WHERE table_type = 'BASE TABLE' ORDER BY 1",
                self.config.database,
                self.dataset()
            ),
            _ => format!(
                "SELECT table_name FROM {}.information_schema.tables WHERE table_schema = {} AND table_type IN ('MANAGED', 'EXTERNAL', 'BASE TABLE') ORDER BY table_name",
                self.quote(&self.config.database),
                literal(self.db_schema())
            ),
        }
    }

    fn columns_sql(&self, table: &str) -> String {
        let (schema, name) = self.split(table);
        match self.kind {
            ConnectorKind::Sqlserver => format!(
                "SELECT CAST(COLUMN_NAME AS NVARCHAR(256)), CAST(DATA_TYPE AS NVARCHAR(128)) FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
                literal(schema.unwrap_or("dbo")),
                literal(name)
            ),
            ConnectorKind::Clickhouse => format!(
                "SELECT name, type FROM system.columns WHERE database = {} AND table = {} ORDER BY position",
                literal(&self.config.database),
                literal(name)
            ),
            ConnectorKind::Snowflake => format!(
                "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = {} AND table_name = {} ORDER BY ordinal_position",
                literal(schema.unwrap_or("PUBLIC")),
                literal(name)
            ),
            ConnectorKind::Bigquery => format!(
                "SELECT column_name, data_type FROM `{}.{}`.INFORMATION_SCHEMA.COLUMNS WHERE table_name = {} ORDER BY ordinal_position",
                self.config.database,
                self.dataset(),
                literal(name)
            ),
            _ => format!(
                "SELECT column_name, data_type FROM {}.information_schema.columns WHERE table_schema = {} AND table_name = {} ORDER BY ordinal_position",
                self.quote(&self.config.database),
                literal(self.db_schema()),
                literal(name)
            ),
        }
    }

    fn select_sql(&self, table: &str, columns: &[String], limit: usize) -> String {
        let projection = columns
            .iter()
            .map(|column| match self.kind {
                ConnectorKind::Sqlserver => format!("CAST({} AS NVARCHAR(MAX))", self.quote(column)),
                _ => self.quote(column),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let from = self.qualified(table);
        if self.kind == ConnectorKind::Sqlserver {
            format!("SELECT TOP ({limit}) {projection} FROM {from}")
        } else {
            format!("SELECT {projection} FROM {from} LIMIT {limit}")
        }
    }

    async fn columns(&self, table: &str) -> Result<Vec<ExploredColumn>, ConnectorError> {
        let result = self.run(&self.columns_sql(table), 10_000).await?;
        let columns = result
            .rows
            .into_iter()
            .map(|row| ExploredColumn {
                name: row.first().cloned().flatten().unwrap_or_default(),
                data_type: row.get(1).cloned().flatten().unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        if columns.is_empty() {
            return Err(ConnectorError::Failed(format!("Table {table} was not found or has no readable columns")));
        }
        Ok(columns)
    }
}

#[async_trait]
impl Connector for RemoteConnector {
    fn kind(&self) -> ConnectorKind {
        self.kind
    }

    async fn test(&self) -> Result<String, ConnectorError> {
        self.run("SELECT 1", 1).await.map_err(|error| ConnectorError::Unavailable(error.to_string()))?;
        Ok(match self.kind {
            ConnectorKind::Sqlserver => "SQL Server connection successful (ApplicationIntent=ReadOnly, SELECT-only)".into(),
            ConnectorKind::Clickhouse => "ClickHouse connection successful (readonly=1)".into(),
            _ => "Connection successful; tables are imported as read-only snapshots".into(),
        })
    }

    async fn list_tables(&self) -> Result<Vec<String>, ConnectorError> {
        if matches!(self.kind, ConnectorKind::Bigquery) && self.dataset().is_empty() {
            return Err(ConnectorError::Unsupported("BigQuery needs a dataset".into()));
        }
        let result = self.run(&self.list_sql(), 100_000).await?;
        Ok(result.rows.into_iter().filter_map(|row| row.into_iter().next().flatten()).collect())
    }

    async fn describe(&self, table: &str) -> Result<ExploredTable, ConnectorError> {
        let columns = self.columns(table).await?;
        let names = columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>();
        let sample = self.run(&self.select_sql(table, &names, 5), 5).await?;
        let samples = sample
            .rows
            .into_iter()
            .map(|row| {
                names
                    .iter()
                    .zip(row)
                    .map(|(name, value)| (name.clone(), value.map_or(Value::Null, Value::String)))
                    .collect::<BTreeMap<_, _>>()
            })
            .collect();
        let count_sql = match self.kind {
            ConnectorKind::Sqlserver => format!("SELECT CAST(COUNT_BIG(*) AS NVARCHAR(40)) FROM {}", self.qualified(table)),
            _ => format!("SELECT COUNT(*) FROM {}", self.qualified(table)),
        };
        let row_count = self
            .run(&count_sql, 1)
            .await
            .ok()
            .and_then(|result| result.rows.into_iter().next())
            .and_then(|row| row.into_iter().next().flatten())
            .and_then(|text| text.parse().ok())
            .unwrap_or(-1);
        Ok(ExploredTable { name: table.to_owned(), row_count, columns, samples })
    }

    async fn fetch(&self, table: &str, row_cap: usize) -> Result<Snapshot, ConnectorError> {
        let columns = self.columns(table).await?;
        let names = columns.into_iter().map(|column| column.name).collect::<Vec<_>>();
        let mut result = self.run(&self.select_sql(table, &names, row_cap + 1), row_cap + 1).await?;
        result.columns = names;
        if result.rows.len() > row_cap {
            result.rows.truncate(row_cap);
            result.truncated = true;
        }
        Ok(result)
    }

    async fn query(&self, sql: &str, row_cap: usize) -> Result<Snapshot, ConnectorError> {
        let mut result = self.run(sql, row_cap + 1).await?;
        if result.rows.len() > row_cap {
            result.rows.truncate(row_cap);
            result.truncated = true;
        }
        Ok(result)
    }
}
