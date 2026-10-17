//! MotherDuck through DuckDB's `md:` ATTACH. The token lives in a TEMPORARY (in-memory) DuckDB secret.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;

use super::sql::{attach_alias, DbConfig};
use super::{redact, Connector, ConnectorError, ConnectorKind, ExploredColumn, ExploredTable};
use crate::engine::duck::{quote_ident, sql_string, Analytics};

pub struct MotherDuck {
    config: DbConfig,
}

impl MotherDuck {
    pub fn new(config: DbConfig) -> Self {
        Self { config }
    }

    fn attach(&self, duck: &Analytics) -> Result<String, ConnectorError> {
        attach_on(duck.connection(), &self.config)
    }

    fn scratch(&self) -> Result<(duckdb::Connection, String), ConnectorError> {
        let conn = duckdb::Connection::open_in_memory()?;
        let alias = attach_on(&conn, &self.config)?;
        Ok((conn, alias))
    }
}

fn attach_on(conn: &duckdb::Connection, config: &DbConfig) -> Result<String, ConnectorError> {
    ensure_extension_on(conn)?;
    let alias = attach_alias(config);
    let wrap = |error: duckdb::Error| ConnectorError::Failed(redact(error, &config.password));
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TEMPORARY SECRET {} (TYPE motherduck, TOKEN {});",
        quote_ident(&format!("{alias}_secret")),
        sql_string(&config.password)
    ))
    .map_err(wrap)?;
    let target = sql_string(&format!("md:{}", config.database));
    let read_only = format!("ATTACH IF NOT EXISTS {target} AS {} (READ_ONLY);", quote_ident(&alias));
    if conn.execute_batch(&read_only).is_err() {
        // Some MotherDuck databases (shares excepted) reject READ_ONLY; every statement we run is a SELECT
        // and a read-scaling token is recommended, but say so instead of silently downgrading.
        conn.execute_batch(&format!("ATTACH IF NOT EXISTS {target} AS {};", quote_ident(&alias))).map_err(wrap)?;
    }
    Ok(alias)
}

fn ensure_extension_on(conn: &duckdb::Connection) -> Result<(), ConnectorError> {
    if conn.execute_batch("LOAD motherduck;").is_ok() || conn.execute_batch("INSTALL motherduck; LOAD motherduck;").is_ok() {
        return Ok(());
    }
    Err(ConnectorError::Unavailable(
        "DuckDB could not load the MotherDuck extension (needs network once to install, and a DuckDB version MotherDuck supports).".into(),
    ))
}

fn split(table: &str) -> (&str, &str) {
    table.split_once('.').unwrap_or(("main", table))
}

#[async_trait]
impl Connector for MotherDuck {
    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Motherduck
    }

    async fn test(&self) -> Result<String, ConnectorError> {
        let (conn, alias) = self.scratch()?;
        conn.query_row(&format!("SELECT count(*) FROM duckdb_tables() WHERE database_name = {}", sql_string(&alias)), [], |row| row.get::<_, i64>(0))?;
        Ok("MotherDuck connection successful (attached read-only; a read-scaling token is recommended)".into())
    }

    async fn list_tables(&self) -> Result<Vec<String>, ConnectorError> {
        let (conn, alias) = self.scratch()?;
        let mut statement = conn.prepare(
            "SELECT schema_name || '.' || table_name FROM duckdb_tables() WHERE database_name = ? ORDER BY 1",
        )?;
        let names = statement.query_map([&alias], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(names)
    }

    async fn describe(&self, table: &str) -> Result<ExploredTable, ConnectorError> {
        let (conn, alias) = self.scratch()?;
        let (schema, name) = split(table);
        let mut statement = conn.prepare(
            "SELECT column_name, data_type FROM duckdb_columns() WHERE database_name = ? AND schema_name = ? AND table_name = ? ORDER BY column_index",
        )?;
        let columns = statement
            .query_map([alias.as_str(), schema, name], |row| Ok(ExploredColumn { name: row.get(0)?, data_type: row.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        if columns.is_empty() {
            return Err(ConnectorError::Failed(format!("Table {table} not found")));
        }
        let qualified = format!("{}.{}.{}", quote_ident(&alias), quote_ident(schema), quote_ident(name));
        let row_count: i64 = conn.query_row(&format!("SELECT count(*) FROM {qualified}"), [], |row| row.get(0))?;
        let projection = columns.iter().map(|c| format!("CAST({} AS VARCHAR)", quote_ident(&c.name))).collect::<Vec<_>>().join(", ");
        let mut sample = conn.prepare(&format!("SELECT {projection} FROM {qualified} LIMIT 5"))?;
        let width = columns.len();
        let samples = sample
            .query_map([], |row| {
                let mut record = BTreeMap::new();
                for index in 0..width {
                    let value: Option<String> = row.get(index)?;
                    record.insert(columns[index].name.clone(), value.map_or(Value::Null, Value::String));
                }
                Ok(record)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ExploredTable { name: table.to_owned(), row_count, columns, samples })
    }

    fn scan(&self, duck: &Analytics, table: &str, relation: &str) -> Result<(), ConnectorError> {
        let alias = self.attach(duck)?;
        let (schema, name) = split(table);
        let reader = format!("{}.{}.{}", quote_ident(&alias), quote_ident(schema), quote_ident(name));
        duck.create_view(relation, &reader).map_err(|error| ConnectorError::Failed(redact(error, &self.config.password)))
    }
}
