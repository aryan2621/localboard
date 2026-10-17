use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{mysql::MySqlConnectOptions, Connection, MySqlConnection, PgConnection, Row, SqliteConnection};
use std::collections::BTreeMap;

pub use super::{ExploredColumn, ExploredTable};
use super::{redact, Connector, ConnectorError, ConnectorKind, Snapshot, DEFAULT_ROW_CAP};
use crate::engine::duck::{quote_ident, sql_string, Analytics};

#[derive(Clone, Serialize, Deserialize)]
pub struct DbConfig {
    pub name: String,
    pub kind: ConnectorKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    /// The one secret of the connection (password, token, key). Lives in the keychain only.
    pub password: String,
    /// Non-secret, kind-specific settings (warehouse, dataset, http_path, row_cap, ...).
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

/// The non-secret part of a database connection, persisted in `connections.config_json`.
#[derive(Clone, Serialize, Deserialize)]
pub struct DbParams {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl DbConfig {
    pub fn params(&self) -> DbParams {
        DbParams {
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone(),
            username: self.username.clone(),
            extra: self.extra.clone(),
        }
    }

    pub fn from_params(name: String, kind: ConnectorKind, params: DbParams, password: String) -> Self {
        Self { name, kind, host: params.host, port: params.port, database: params.database, username: params.username, password, extra: params.extra }
    }

    pub fn extra(&self, key: &str) -> &str {
        self.extra.get(key).map_or("", |value| value.trim())
    }

    /// Row cap for snapshot imports (default 1M, at most 10M).
    pub fn row_cap(&self) -> usize {
        self.extra("row_cap").parse::<usize>().ok().filter(|cap| *cap > 0).map_or(DEFAULT_ROW_CAP, |cap| cap.min(10_000_000))
    }

    /// Trims, applies per-kind defaults and rejects configurations that cannot work.
    pub fn normalized(mut self) -> Result<Self, ConnectorError> {
        self.name = self.name.trim().to_owned();
        self.database = self.database.trim().to_owned();
        self.host = self.host.trim().to_owned();
        if self.name.is_empty() {
            return Err(ConnectorError::Unsupported("Name is required".into()));
        }
        match self.kind {
            ConnectorKind::Clickhouse if self.database.is_empty() => self.database = "default".into(),
            ConnectorKind::Databricks if self.database.is_empty() => self.database = "main".into(),
            ConnectorKind::Sqlserver if self.database.is_empty() => self.database = "master".into(),
            _ => {}
        }
        if self.database.is_empty() {
            return Err(ConnectorError::Unsupported("Database (or project / catalog) is required".into()));
        }
        let needs_host = !matches!(self.kind, ConnectorKind::Sqlite | ConnectorKind::Motherduck | ConnectorKind::Bigquery);
        if needs_host && self.host.is_empty() {
            return Err(ConnectorError::Unsupported("Host is required".into()));
        }
        Ok(self)
    }
}

/// Postgres, MySQL, SQLite, Supabase and Redshift over sqlx. Sessions are forced read-only at connect time.
pub struct SqlConnector {
    config: DbConfig,
}

impl SqlConnector {
    pub fn new(config: DbConfig) -> Result<Self, ConnectorError> {
        match config.kind {
            ConnectorKind::Postgres | ConnectorKind::Mysql | ConnectorKind::Sqlite | ConnectorKind::Supabase | ConnectorKind::Redshift => {
                Ok(Self { config })
            }
            _ => Err(ConnectorError::Unsupported("Not a sqlx database connector kind".into())),
        }
    }
}

#[async_trait]
impl Connector for SqlConnector {
    fn kind(&self) -> ConnectorKind {
        self.config.kind
    }

    async fn test(&self) -> Result<String, ConnectorError> {
        test(&self.config).await.map_err(ConnectorError::Unavailable)
    }

    async fn list_tables(&self) -> Result<Vec<String>, ConnectorError> {
        Ok(self.explore().await?.into_iter().map(|table| table.name).collect())
    }

    async fn describe(&self, table: &str) -> Result<ExploredTable, ConnectorError> {
        self.explore()
            .await?
            .into_iter()
            .find(|candidate| candidate.name == table)
            .ok_or_else(|| ConnectorError::Failed(format!("Table {table} not found")))
    }

    async fn explore(&self) -> Result<Vec<ExploredTable>, ConnectorError> {
        explore(&self.config).await.map_err(ConnectorError::Failed)
    }

    async fn fetch(&self, table: &str, row_cap: usize) -> Result<Snapshot, ConnectorError> {
        if self.config.kind != ConnectorKind::Redshift {
            return Err(ConnectorError::Unsupported("This connector reads tables live; there is nothing to snapshot".into()));
        }
        redshift_fetch(&self.config, table, row_cap).await.map_err(ConnectorError::Failed)
    }

    fn scan(&self, duck: &Analytics, table: &str, relation: &str) -> Result<(), ConnectorError> {
        if self.config.kind == ConnectorKind::Redshift {
            return Err(ConnectorError::Unsupported("Redshift imports snapshots instead of live views".into()));
        }
        register_scan(duck, &self.config, table, relation)
    }
}

async fn test(config: &DbConfig) -> Result<String, String> {
    match config.kind {
        ConnectorKind::Postgres | ConnectorKind::Supabase | ConnectorKind::Redshift => {
            let _connection = connect_postgres(config).await?;
            Ok("Connection successful (read-only transaction)".into())
        }
        ConnectorKind::Mysql => {
            let _connection = connect_mysql(config).await?;
            Ok("MySQL connection successful; use a SELECT-only account".into())
        }
        ConnectorKind::Sqlite => {
            let _connection = connect_sqlite(config).await?;
            Ok("SQLite opened read-only".into())
        }
        _ => Err("Unsupported database engine".into()),
    }
}

async fn explore(config: &DbConfig) -> Result<Vec<ExploredTable>, String> {
    match config.kind {
        ConnectorKind::Postgres | ConnectorKind::Supabase | ConnectorKind::Redshift => explore_postgres(config).await,
        ConnectorKind::Mysql => explore_mysql(config).await,
        ConnectorKind::Sqlite => explore_sqlite(config).await,
        _ => Err("Unsupported database engine".into()),
    }
}

async fn connect_postgres(config: &DbConfig) -> Result<PgConnection, String> {
    let mut options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.username)
        .password(&config.password)
        .database(&config.database);
    let strict = config.kind == ConnectorKind::Postgres;
    if strict {
        options = options.options([("default_transaction_read_only", "on"), ("statement_timeout", "30000")]);
    }
    if config.kind == ConnectorKind::Supabase {
        options = options.ssl_mode(PgSslMode::Require);
    }
    let mut connection = sqlx::PgConnection::connect_with(&options)
        .await
        .map_err(|error| redact(error, &config.password))?;
    if strict {
        let read_only: String = sqlx::query_scalar("SHOW transaction_read_only")
            .fetch_one(&mut connection)
            .await
            .map_err(|error| redact(error, &config.password))?;
        if read_only != "on" {
            return Err("PostgreSQL read-only mode not active".into());
        }
    } else {
        // Poolers and Redshift reject startup options; a never-committed READ ONLY transaction is the guard.
        sqlx::query("BEGIN READ ONLY")
            .execute(&mut connection)
            .await
            .map_err(|error| format!("Could not start a read-only transaction: {}", redact(error, &config.password)))?;
    }
    Ok(connection)
}

/// Copies up to `cap` rows of a Postgres-protocol table as text (Redshift snapshots).
async fn redshift_fetch(config: &DbConfig, table: &str, cap: usize) -> Result<Snapshot, String> {
    let (schema, name) = table.split_once('.').unwrap_or(("public", table));
    let mut connection = connect_postgres(config).await?;
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
    )
    .bind(schema)
    .bind(name)
    .fetch_all(&mut connection)
    .await
    .map_err(|error| redact(error, &config.password))?;
    if columns.is_empty() {
        return Err(format!("Table {table} has no readable columns"));
    }
    let projection = columns.iter().map(|column| format!("{}::varchar", quote_pg(column))).collect::<Vec<_>>().join(", ");
    let sql = format!("SELECT {projection} FROM {}.{} LIMIT {}", quote_pg(schema), quote_pg(name), cap + 1);
    let fetched = sqlx::query(&sql).fetch_all(&mut connection).await.map_err(|error| redact(error, &config.password))?;
    let mut rows = Vec::with_capacity(fetched.len().min(cap));
    for row in fetched.iter().take(cap) {
        let mut values = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            values.push(row.try_get::<Option<String>, _>(index).map_err(|error| redact(error, &config.password))?);
        }
        rows.push(values);
    }
    Ok(Snapshot { columns, rows, truncated: fetched.len() > cap })
}

async fn connect_mysql(config: &DbConfig) -> Result<MySqlConnection, String> {
    let options = MySqlConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.username)
        .password(&config.password)
        .database(&config.database);
    let mut connection = sqlx::MySqlConnection::connect_with(&options)
        .await
        .map_err(|error| redact(error, &config.password))?;
    sqlx::query("SET SESSION TRANSACTION READ ONLY")
        .execute(&mut connection)
        .await
        .map_err(|error| redact(error, &config.password))?;
    sqlx::query("SET SESSION MAX_EXECUTION_TIME = 30000")
        .execute(&mut connection)
        .await
        .map_err(|error| redact(error, &config.password))?;
    Ok(connection)
}

async fn connect_sqlite(config: &DbConfig) -> Result<SqliteConnection, String> {
    let options = SqliteConnectOptions::new()
        .filename(&config.database)
        .read_only(true)
        .create_if_missing(false);
    sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| redact(error, &config.password))
}

macro_rules! count_i64 {
    ($connection:expr, $sql:expr, $secret:expr) => {
        sqlx::query_scalar::<_, i64>($sql)
            .fetch_one(&mut *$connection)
            .await
            .map_err(|error| redact(error, $secret))
    };
}

macro_rules! sample_rows {
    ($connection:expr, $sql:expr, $columns:expr, $secret:expr) => {{
        let columns: &[ExploredColumn] = $columns;
        let sql: &str = $sql;
        let result: Result<Vec<BTreeMap<String, Value>>, String> = if columns.is_empty() || sql.is_empty() {
            Ok(Vec::new())
        } else {
            let rows = sqlx::query(sql)
                .fetch_all(&mut *$connection)
                .await
                .map_err(|error| redact(error, $secret))?;
            let mut samples = Vec::with_capacity(rows.len());
            for row in rows {
                let mut record = BTreeMap::new();
                for (index, column) in columns.iter().enumerate() {
                    let value: Option<String> = row.try_get(index).map_err(|error| redact(error, $secret))?;
                    record.insert(column.name.clone(), value.map(Value::String).unwrap_or(Value::Null));
                }
                samples.push(record);
            }
            Ok(samples)
        };
        result
    }};
}

async fn explore_postgres(config: &DbConfig) -> Result<Vec<ExploredTable>, String> {
    let mut connection = connect_postgres(config).await?;
    let tables: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_schema, table_name
         FROM information_schema.tables
         WHERE table_type = 'BASE TABLE'
           AND table_schema NOT IN ('pg_catalog', 'information_schema')
         ORDER BY table_schema, table_name",
    )
    .fetch_all(&mut connection)
    .await
    .map_err(|error| redact(error, &config.password))?;
    let mut explored = Vec::new();
    for (schema, table) in tables {
        let columns: Vec<(String, String)> = sqlx::query_as(
            "SELECT column_name, data_type
             FROM information_schema.columns
             WHERE table_schema = $1 AND table_name = $2
             ORDER BY ordinal_position",
        )
        .bind(&schema)
        .bind(&table)
        .fetch_all(&mut connection)
        .await
        .map_err(|error| redact(error, &config.password))?;
        let columns = columns
            .into_iter()
            .map(|(name, data_type)| ExploredColumn { name, data_type })
            .collect::<Vec<_>>();
        let qualified = format!("{}.{}", quote_pg(&schema), quote_pg(&table));
        let count_sql = format!("SELECT COUNT(*)::bigint FROM {qualified}");
        let sample_sql = pg_sample_sql(&qualified, &columns);
        let row_count = count_i64!(&mut connection, &count_sql, &config.password)?;
        let samples = sample_rows!(&mut connection, &sample_sql, &columns, &config.password)?;
        explored.push(ExploredTable {
            name: format!("{schema}.{table}"),
            row_count,
            columns,
            samples,
        });
    }
    Ok(explored)
}

async fn explore_mysql(config: &DbConfig) -> Result<Vec<ExploredTable>, String> {
    let mut connection = connect_mysql(config).await?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = ? AND table_type = 'BASE TABLE'
         ORDER BY table_name",
    )
    .bind(&config.database)
    .fetch_all(&mut connection)
    .await
    .map_err(|error| redact(error, &config.password))?;
    let mut explored = Vec::new();
    for table in tables {
        let columns: Vec<(String, String)> = sqlx::query_as(
            "SELECT column_name, data_type FROM information_schema.columns
             WHERE table_schema = ? AND table_name = ?
             ORDER BY ordinal_position",
        )
        .bind(&config.database)
        .bind(&table)
        .fetch_all(&mut connection)
        .await
        .map_err(|error| redact(error, &config.password))?;
        let columns = columns
            .into_iter()
            .map(|(name, data_type)| ExploredColumn { name, data_type })
            .collect::<Vec<_>>();
        let qualified = format!("{}.{}", quote_mysql(&config.database), quote_mysql(&table));
        let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM {qualified}");
        let sample_sql = mysql_sample_sql(&qualified, &columns);
        let row_count = count_i64!(&mut connection, &count_sql, &config.password)?;
        let samples = sample_rows!(&mut connection, &sample_sql, &columns, &config.password)?;
        explored.push(ExploredTable {
            name: table,
            row_count,
            columns,
            samples,
        });
    }
    Ok(explored)
}

async fn explore_sqlite(config: &DbConfig) -> Result<Vec<ExploredTable>, String> {
    let mut connection = connect_sqlite(config).await?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut connection)
    .await
    .map_err(|error| redact(error, &config.password))?;
    let mut explored = Vec::new();
    for table in tables {
        let quoted = quote_pg(&table);
        let info = sqlx::query(&format!("PRAGMA table_info({quoted})"))
            .fetch_all(&mut connection)
            .await
            .map_err(|error| redact(error, &config.password))?;
        let mut columns = Vec::new();
        for row in info {
            columns.push(ExploredColumn {
                name: row.try_get(1).map_err(|error| redact(error, &config.password))?,
                data_type: row.try_get::<String, _>(2).unwrap_or_default(),
            });
        }
        let count_sql = format!("SELECT COUNT(*) FROM {quoted}");
        let sample_sql = sqlite_sample_sql(&quoted, &columns);
        let row_count = count_i64!(&mut connection, &count_sql, &config.password)?;
        let samples = sample_rows!(&mut connection, &sample_sql, &columns, &config.password)?;
        explored.push(ExploredTable {
            name: table,
            row_count,
            columns,
            samples,
        });
    }
    Ok(explored)
}

fn pg_sample_sql(qualified: &str, columns: &[ExploredColumn]) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let projection = columns
        .iter()
        .map(|column| format!("{}::text", quote_pg(&column.name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {projection} FROM {qualified} LIMIT 5")
}

fn mysql_sample_sql(qualified: &str, columns: &[ExploredColumn]) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let projection = columns
        .iter()
        .map(|column| format!("CAST({} AS CHAR)", quote_mysql(&column.name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {projection} FROM {qualified} LIMIT 5")
}

fn sqlite_sample_sql(qualified: &str, columns: &[ExploredColumn]) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let projection = columns
        .iter()
        .map(|column| format!("CAST({} AS TEXT)", quote_pg(&column.name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {projection} FROM {qualified} LIMIT 5")
}

fn quote_pg(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_mysql(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn ident_part(name: &str) -> Result<&str, ConnectorError> {
    if name.is_empty()
        || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(ConnectorError::Unsupported("That table name is not safe to register".into()));
    }
    Ok(name)
}

fn split_ref(source_ref: &str) -> Result<(String, String), ConnectorError> {
    match source_ref.split_once('.') {
        Some((schema, table)) => Ok((ident_part(schema)?.to_owned(), ident_part(table)?.to_owned())),
        None => Ok(("public".into(), ident_part(source_ref)?.to_owned())),
    }
}

pub(super) fn ensure_extension(duck: &Analytics, names: &[&str]) -> Result<(), ConnectorError> {
    let conn = duck.connection();
    for name in names {
        if conn.execute_batch(&format!("LOAD {name};")).is_ok() {
            return Ok(());
        }
        if conn.execute_batch(&format!("INSTALL {name}; LOAD {name};")).is_ok() {
            return Ok(());
        }
        if conn.execute_batch(&format!("INSTALL {name} FROM community; LOAD {name};")).is_ok() {
            return Ok(());
        }
    }
    Err(ConnectorError::Unavailable(format!(
        "DuckDB could not load a read-only scanner (tried {}). Stay online once so the extension can install, then try again. LocalBoard will not copy the table.",
        names.join(", ")
    )))
}

/// A stable, unique DuckDB alias for one connection, so two connections never share an attachment.
pub(super) fn attach_alias(config: &DbConfig) -> String {
    use sha2::{Digest, Sha256};
    let key = format!(
        "{:?}|{}|{}|{}|{}|{}",
        config.kind, config.name, config.host, config.port, config.database, config.username
    );
    let digest = Sha256::digest(key.as_bytes());
    format!("lb_{}", digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect::<String>())
}

/// Credentials go into a TEMPORARY secret, which DuckDB keeps in memory only. The persisted view text
/// then holds an alias and a table name, never a password.
fn create_secret(duck: &Analytics, config: &DbConfig, alias: &str, secret_type: &str) -> Result<(), ConnectorError> {
    let statement = format!(
        "CREATE OR REPLACE TEMPORARY SECRET {} (TYPE {secret_type}, HOST {}, PORT {}, DATABASE {}, USER {}, PASSWORD {});",
        quote_ident(&format!("{alias}_secret")),
        sql_string(&config.host),
        config.port,
        sql_string(&config.database),
        sql_string(&config.username),
        sql_string(&config.password),
    );
    duck.connection().execute_batch(&statement).map_err(|error| ConnectorError::Failed(redact(error, &config.password)))
}

fn attach_remote(duck: &Analytics, config: &DbConfig, alias: &str, secret_type: &str) -> Result<(), ConnectorError> {
    create_secret(duck, config, alias, secret_type)?;
    let attach = format!(
        "ATTACH IF NOT EXISTS '' AS {} (TYPE {secret_type}, SECRET {}, READ_ONLY);",
        quote_ident(alias),
        quote_ident(&format!("{alias}_secret"))
    );
    duck.connection().execute_batch(&attach).map_err(|error| ConnectorError::Failed(redact(error, &config.password)))
}

/// Registers a live read-only DuckDB view over a remote table. Failures are loud; nothing is copied.
pub fn register_scan(duck: &Analytics, config: &DbConfig, source_ref: &str, relation: &str) -> Result<(), ConnectorError> {
    let (schema, table) = match config.kind {
        ConnectorKind::Mysql | ConnectorKind::Sqlite => (String::new(), ident_part(source_ref)?.to_owned()),
        ConnectorKind::Postgres | ConnectorKind::Supabase => split_ref(source_ref)?,
        _ => return Err(ConnectorError::Unsupported("Not a database connector".into())),
    };
    let alias = attach_alias(config);
    match config.kind {
        ConnectorKind::Postgres | ConnectorKind::Supabase => {
            ensure_extension(duck, &["postgres", "postgres_scanner"])?;
            attach_remote(duck, config, &alias, "postgres")?;
            let reader = format!("{}.{}.{}", quote_ident(&alias), quote_ident(&schema), quote_ident(&table));
            duck.create_view(relation, &reader).map_err(|error| ConnectorError::Failed(redact(error, &config.password)))
        }
        ConnectorKind::Mysql => {
            ensure_extension(duck, &["mysql", "mysql_scanner"])?;
            attach_remote(duck, config, &alias, "mysql")?;
            let reader = format!("{}.{}.{}", quote_ident(&alias), quote_ident(&config.database), quote_ident(&table));
            duck.create_view(relation, &reader).map_err(|error| ConnectorError::Failed(redact(error, &config.password)))
        }
        ConnectorKind::Sqlite => {
            let attach = format!(
                "ATTACH IF NOT EXISTS {} AS {} (TYPE SQLITE, READ_ONLY);",
                sql_string(&config.database),
                quote_ident(&alias)
            );
            duck.connection()
                .execute_batch(&attach)
                .map_err(|error| ConnectorError::Failed(redact(error, &config.password)))?;
            let reader = format!("{}.{}", quote_ident(&alias), quote_ident(&table));
            duck.create_view(relation, &reader).map_err(|error| ConnectorError::Failed(redact(error, &config.password)))
        }
        _ => Err(ConnectorError::Unsupported("Not a database connector".into())),
    }
}
