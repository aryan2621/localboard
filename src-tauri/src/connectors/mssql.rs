//! SQL Server over TDS (tiberius). Connects with ApplicationIntent=ReadOnly; only text columns are read,
//! the caller casts everything to NVARCHAR.

use async_trait::async_trait;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use super::remote::Backend;
use super::sql::DbConfig;
use super::Snapshot;

pub struct SqlServer {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    trust_cert: bool,
}

impl SqlServer {
    pub fn new(config: &DbConfig) -> Self {
        Self {
            host: config.host.clone(),
            port: if config.port == 0 { 1433 } else { config.port },
            database: config.database.clone(),
            user: config.username.clone(),
            password: config.password.clone(),
            trust_cert: config.extra("trust_server_certificate") == "true",
        }
    }
}

#[async_trait]
impl Backend for SqlServer {
    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, String> {
        let mut config = Config::new();
        config.host(&self.host);
        config.port(self.port);
        config.database(&self.database);
        config.authentication(AuthMethod::sql_server(&self.user, &self.password));
        config.readonly(true);
        config.encryption(EncryptionLevel::Required);
        if self.trust_cert {
            config.trust_cert();
        }
        let tcp = tokio::time::timeout(std::time::Duration::from_secs(15), TcpStream::connect(config.get_addr()))
            .await
            .map_err(|_| "Timed out connecting to SQL Server".to_owned())?
            .map_err(|error| error.to_string())?;
        tcp.set_nodelay(true).map_err(|error| error.to_string())?;
        let mut client = Client::connect(config, tcp.compat_write()).await.map_err(|error| error.to_string())?;
        let rows = client
            .simple_query(sql)
            .await
            .map_err(|error| error.to_string())?
            .into_first_result()
            .await
            .map_err(|error| error.to_string())?;
        let mut columns = Vec::new();
        let mut out = Vec::new();
        for row in rows.iter().take(max_rows) {
            if columns.is_empty() {
                columns = row.columns().iter().map(|column| column.name().to_owned()).collect();
            }
            let mut values = Vec::with_capacity(row.columns().len());
            for index in 0..row.columns().len() {
                values.push(row.try_get::<&str, _>(index).map_err(|error| error.to_string())?.map(str::to_owned));
            }
            out.push(values);
        }
        Ok(Snapshot { columns, rows: out, truncated: false })
    }
}
