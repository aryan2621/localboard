//! Databricks SQL Statement Execution API with a personal access token. SELECT-only, inline JSON results.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::remote::{cell, Backend};
use super::sql::DbConfig;
use super::{ConnectorError, Snapshot};

pub struct Databricks {
    client: reqwest::Client,
    base: String,
    token: String,
    warehouse: String,
    catalog: String,
}

fn warehouse_id(http_path: &str) -> Option<String> {
    let id = http_path.trim().trim_end_matches('/').rsplit('/').next()?;
    (!id.is_empty() && id.chars().all(|ch| ch.is_ascii_alphanumeric())).then(|| id.to_owned())
}

impl Databricks {
    pub fn new(config: &DbConfig) -> Result<Self, ConnectorError> {
        let host = config.host.trim().trim_start_matches("https://").trim_end_matches('/');
        if host.is_empty() || !host.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-') {
            return Err(ConnectorError::Unsupported("Host must be the workspace hostname, e.g. dbc-1234.cloud.databricks.com".into()));
        }
        let warehouse = warehouse_id(config.extra("http_path"))
            .ok_or_else(|| ConnectorError::Unsupported("HTTP path must end in the SQL warehouse id (/sql/1.0/warehouses/<id>)".into()))?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|error| ConnectorError::Failed(error.to_string()))?;
        Ok(Self { client, base: format!("https://{host}"), token: config.password.clone(), warehouse, catalog: config.database.clone() })
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        read(response).await
    }
}

async fn read(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("Databricks returned {status}: {}", text.chars().take(400).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|error| format!("Unreadable Databricks response: {error}"))
}

fn rows_of(value: &Value) -> Vec<Vec<Option<String>>> {
    value
        .as_array()
        .map(|rows| rows.iter().map(|row| row.as_array().map(|cells| cells.iter().map(cell).collect()).unwrap_or_default()).collect())
        .unwrap_or_default()
}

#[async_trait]
impl Backend for Databricks {
    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, String> {
        let response = self
            .client
            .post(format!("{}/api/2.0/sql/statements", self.base))
            .bearer_auth(&self.token)
            .json(&json!({
                "warehouse_id": self.warehouse,
                "statement": sql,
                "catalog": self.catalog,
                "wait_timeout": "30s",
                "on_wait_timeout": "CONTINUE",
                "format": "JSON_ARRAY",
                "disposition": "INLINE",
                "row_limit": max_rows,
            }))
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        let mut state = read(response).await?;
        let id = state["statement_id"].as_str().unwrap_or_default().to_owned();
        for _ in 0..240 {
            match state["status"]["state"].as_str().unwrap_or_default() {
                "PENDING" | "RUNNING" => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    state = self.get(&format!("/api/2.0/sql/statements/{id}")).await?;
                }
                "SUCCEEDED" => break,
                other => {
                    let message = state["status"]["error"]["message"].as_str().unwrap_or("statement did not succeed");
                    return Err(format!("Databricks statement {other}: {message}"));
                }
            }
        }
        if state["status"]["state"] != "SUCCEEDED" {
            return Err("Databricks statement timed out".into());
        }
        let columns = state["manifest"]["schema"]["columns"]
            .as_array()
            .map(|columns| columns.iter().map(|column| column["name"].as_str().unwrap_or_default().to_owned()).collect())
            .unwrap_or_default();
        let mut rows = rows_of(&state["result"]["data_array"]);
        let mut next = state["result"]["next_chunk_index"].as_u64();
        while let (Some(index), true) = (next, rows.len() < max_rows) {
            let chunk = self.get(&format!("/api/2.0/sql/statements/{id}/result/chunks/{index}")).await?;
            rows.extend(rows_of(&chunk["data_array"]));
            next = chunk["next_chunk_index"].as_u64();
        }
        rows.truncate(max_rows);
        Ok(Snapshot { columns, rows, truncated: false })
    }
}
