//! ClickHouse over its HTTP interface. Every request carries `readonly=1`.

use async_trait::async_trait;
use serde_json::Value;

use super::remote::{cell, Backend};
use super::sql::DbConfig;
use super::{ConnectorError, Snapshot};

pub struct ClickHouse {
    client: reqwest::Client,
    url: String,
    database: String,
    user: String,
    password: String,
}

impl ClickHouse {
    pub fn new(config: &DbConfig) -> Result<Self, ConnectorError> {
        if config.host.contains('/') || config.host.contains(' ') {
            return Err(ConnectorError::Unsupported("Host must be a bare hostname".into()));
        }
        let secure = config.extra("secure") == "true";
        let port = if config.port == 0 { if secure { 8443 } else { 8123 } } else { config.port };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|error| ConnectorError::Failed(error.to_string()))?;
        Ok(Self {
            client,
            url: format!("{}://{}:{port}/", if secure { "https" } else { "http" }, config.host),
            database: config.database.clone(),
            user: if config.username.is_empty() { "default".into() } else { config.username.clone() },
            password: config.password.clone(),
        })
    }
}

#[async_trait]
impl Backend for ClickHouse {
    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, String> {
        let body = format!("{} FORMAT JSONCompact", sql.trim().trim_end_matches(';'));
        let response = self
            .client
            .post(&self.url)
            .query(&[("database", self.database.as_str()), ("readonly", "1")])
            .basic_auth(&self.user, Some(&self.password))
            .body(body)
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        let status = response.status();
        let text = response.text().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("ClickHouse returned {status}: {}", text.chars().take(400).collect::<String>()));
        }
        let parsed: Value = serde_json::from_str(&text).map_err(|error| format!("Unreadable ClickHouse response: {error}"))?;
        let columns = parsed["meta"]
            .as_array()
            .map(|meta| meta.iter().map(|column| column["name"].as_str().unwrap_or_default().to_owned()).collect())
            .unwrap_or_default();
        let rows = parsed["data"]
            .as_array()
            .map(|data| {
                data.iter()
                    .take(max_rows)
                    .map(|row| row.as_array().map(|cells| cells.iter().map(cell).collect()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Snapshot { columns, rows, truncated: false })
    }
}
