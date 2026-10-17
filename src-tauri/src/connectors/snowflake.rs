//! Snowflake SQL API v2 with a programmatic access token or an RSA key-pair JWT.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::jwt;
use super::remote::{cell, Backend};
use super::sql::DbConfig;
use super::{ConnectorError, Snapshot};

pub struct Snowflake {
    client: reqwest::Client,
    base: String,
    token: String,
    token_type: &'static str,
    database: String,
    schema: String,
    warehouse: String,
    role: String,
}

impl Snowflake {
    pub fn new(config: &DbConfig) -> Result<Self, ConnectorError> {
        let account = config.host.trim().trim_start_matches("https://").trim_end_matches('/');
        let account = account.strip_suffix(".snowflakecomputing.com").unwrap_or(account);
        if account.is_empty() || !account.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_') {
            return Err(ConnectorError::Unsupported("Account must look like xy12345.us-east-1".into()));
        }
        let (token, token_type) = if config.extra("auth") == "keypair" {
            let key = jwt::parse_key(&config.password).map_err(ConnectorError::Unsupported)?;
            let account_name = account.split('.').next().unwrap_or(account).to_ascii_uppercase();
            let subject = format!("{account_name}.{}", config.username.to_ascii_uppercase());
            let now = jwt::now_secs();
            let claims = json!({
                "iss": format!("{subject}.{}", jwt::snowflake_fingerprint(&key).map_err(ConnectorError::Unsupported)?),
                "sub": subject,
                "iat": now,
                "exp": now + 3000,
            });
            (jwt::sign_rs256(&key, &claims).map_err(ConnectorError::Failed)?, "KEYPAIR_JWT")
        } else {
            (config.password.clone(), "PROGRAMMATIC_ACCESS_TOKEN")
        };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|error| ConnectorError::Failed(error.to_string()))?;
        Ok(Self {
            client,
            base: format!("https://{account}.snowflakecomputing.com/api/v2/statements"),
            token,
            token_type,
            database: config.database.clone(),
            schema: config.extra("schema").to_owned(),
            warehouse: config.extra("warehouse").to_owned(),
            role: config.extra("role").to_owned(),
        })
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .bearer_auth(&self.token)
            .header("X-Snowflake-Authorization-Token-Type", self.token_type)
            .header("Accept", "application/json")
            .header("User-Agent", "LocalBoard/0.1")
    }
}

async fn read(response: reqwest::Response) -> Result<(u16, Value), String> {
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("Snowflake returned {status}: {}", text.chars().take(400).collect::<String>()));
    }
    Ok((status.as_u16(), serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn rows_of(value: &Value, out: &mut Vec<Vec<Option<String>>>) {
    if let Some(rows) = value.as_array() {
        out.extend(rows.iter().map(|row| row.as_array().map(|cells| cells.iter().map(cell).collect()).unwrap_or_default()));
    }
}

#[async_trait]
impl Backend for Snowflake {
    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, String> {
        let mut body = json!({ "statement": sql, "timeout": 120, "database": self.database });
        for (key, value) in [("schema", &self.schema), ("warehouse", &self.warehouse), ("role", &self.role)] {
            if !value.is_empty() {
                body[key] = json!(value);
            }
        }
        let response = self.authorized(self.client.post(&self.base)).json(&body).send().await.map_err(|error| error.without_url().to_string())?;
        let (mut status, mut result) = read(response).await?;
        let handle = result["statementHandle"].as_str().unwrap_or_default().to_owned();
        let mut attempts = 0;
        while status == 202 {
            attempts += 1;
            if attempts > 120 {
                return Err("Snowflake statement timed out".into());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let response = self
                .authorized(self.client.get(format!("{}/{handle}", self.base)))
                .send()
                .await
                .map_err(|error| error.without_url().to_string())?;
            (status, result) = read(response).await?;
        }
        let meta = &result["resultSetMetaData"];
        let columns = meta["rowType"]
            .as_array()
            .map(|types| types.iter().map(|column| column["name"].as_str().unwrap_or_default().to_owned()).collect())
            .unwrap_or_default();
        let mut rows = Vec::new();
        rows_of(&result["data"], &mut rows);
        let partitions = meta["partitionInfo"].as_array().map_or(0, Vec::len);
        for partition in 1..partitions {
            if rows.len() >= max_rows {
                break;
            }
            let response = self
                .authorized(self.client.get(format!("{}/{handle}", self.base)))
                .query(&[("partition", partition.to_string())])
                .send()
                .await
                .map_err(|error| error.without_url().to_string())?;
            let (_, page) = read(response).await?;
            rows_of(&page["data"], &mut rows);
        }
        rows.truncate(max_rows);
        Ok(Snapshot { columns, rows, truncated: false })
    }
}
