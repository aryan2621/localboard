//! BigQuery REST `jobs.query`, authenticated with a service-account JSON (JWT exchanged for an OAuth token).
//! Uses the read-only BigQuery scope, so the account cannot write even if the SQL could.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::jwt;
use super::remote::{cell, Backend};
use super::sql::DbConfig;
use super::{ConnectorError, Snapshot};

const SCOPE: &str = "https://www.googleapis.com/auth/bigquery.readonly";
const API: &str = "https://bigquery.googleapis.com/bigquery/v2/projects";

pub struct BigQuery {
    client: reqwest::Client,
    project: String,
    location: String,
    email: String,
    token_uri: String,
    key: rsa::RsaPrivateKey,
    token: Mutex<Option<(String, u64)>>,
}

impl BigQuery {
    pub fn new(config: &DbConfig) -> Result<Self, ConnectorError> {
        let account: Value = serde_json::from_str(&config.password)
            .map_err(|_| ConnectorError::Unsupported("The service-account key must be the JSON file downloaded from Google Cloud".into()))?;
        let field = |name: &str| account[name].as_str().map(str::to_owned);
        let (Some(email), Some(pem)) = (field("client_email"), field("private_key")) else {
            return Err(ConnectorError::Unsupported("Service-account JSON needs client_email and private_key".into()));
        };
        let token_uri = field("token_uri").unwrap_or_else(|| "https://oauth2.googleapis.com/token".into());
        if !token_uri.starts_with("https://") {
            return Err(ConnectorError::Unsupported("token_uri must be https".into()));
        }
        let project = if config.database.is_empty() { field("project_id").unwrap_or_default() } else { config.database.clone() };
        if project.is_empty() || !project.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == ':' || ch == '.') {
            return Err(ConnectorError::Unsupported("Project id is required".into()));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|error| ConnectorError::Failed(error.to_string()))?;
        Ok(Self {
            client,
            project,
            location: config.extra("location").to_owned(),
            email,
            token_uri,
            key: jwt::parse_key(&pem).map_err(ConnectorError::Unsupported)?,
            token: Mutex::new(None),
        })
    }

    async fn access_token(&self) -> Result<String, String> {
        let mut cached = self.token.lock().await;
        let now = jwt::now_secs();
        if let Some((token, expires)) = cached.as_ref() {
            if *expires > now + 60 {
                return Ok(token.clone());
            }
        }
        let assertion = jwt::sign_rs256(
            &self.key,
            &json!({ "iss": self.email, "scope": SCOPE, "aud": self.token_uri, "iat": now, "exp": now + 3000 }),
        )?;
        let response = self
            .client
            .post(&self.token_uri)
            .form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", assertion.as_str())])
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        let value = read(response).await?;
        let token = value["access_token"].as_str().ok_or("Google did not return an access token")?.to_owned();
        *cached = Some((token.clone(), now + value["expires_in"].as_u64().unwrap_or(3000)));
        Ok(token)
    }
}

async fn read(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("Google returned {status}: {}", text.chars().take(400).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|error| format!("Unreadable BigQuery response: {error}"))
}

fn rows_of(value: &Value, out: &mut Vec<Vec<Option<String>>>) {
    if let Some(rows) = value["rows"].as_array() {
        out.extend(rows.iter().map(|row| row["f"].as_array().map(|cells| cells.iter().map(|c| cell(&c["v"])).collect()).unwrap_or_default()));
    }
}

#[async_trait]
impl Backend for BigQuery {
    async fn run(&self, sql: &str, max_rows: usize) -> Result<Snapshot, String> {
        let token = self.access_token().await?;
        let mut body = json!({ "query": sql, "useLegacySql": false, "timeoutMs": 60000, "maxResults": max_rows.min(10_000) });
        if !self.location.is_empty() {
            body["location"] = json!(self.location);
        }
        let response = self
            .client
            .post(format!("{API}/{}/queries", self.project))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        let mut page = read(response).await?;
        let job = page["jobReference"]["jobId"].as_str().unwrap_or_default().to_owned();
        let location = page["jobReference"]["location"].as_str().unwrap_or_default().to_owned();
        let mut attempts = 0;
        let mut columns: Vec<String> = Vec::new();
        let mut rows = Vec::new();
        let mut first = true;
        loop {
            if page["jobComplete"] == false {
                attempts += 1;
                if attempts > 120 {
                    return Err("BigQuery job timed out".into());
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            } else {
                if first {
                    columns = page["schema"]["fields"]
                        .as_array()
                        .map(|fields| fields.iter().map(|field| field["name"].as_str().unwrap_or_default().to_owned()).collect())
                        .unwrap_or_default();
                    first = false;
                }
                rows_of(&page, &mut rows);
                match page["pageToken"].as_str() {
                    Some(_) if rows.len() >= max_rows => break,
                    None => break,
                    Some(_) => {}
                }
            }
            let mut query = vec![("maxResults", "10000".to_owned())];
            if let Some(next) = page["pageToken"].as_str() {
                query.push(("pageToken", next.to_owned()));
            }
            if !location.is_empty() {
                query.push(("location", location.clone()));
            }
            let response = self
                .client
                .get(format!("{API}/{}/queries/{job}", self.project))
                .bearer_auth(&token)
                .query(&query)
                .send()
                .await
                .map_err(|error| error.without_url().to_string())?;
            page = read(response).await?;
        }
        rows.truncate(max_rows);
        Ok(Snapshot { columns, rows, truncated: false })
    }
}
