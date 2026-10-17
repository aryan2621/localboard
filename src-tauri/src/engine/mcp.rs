//! MCP JSON-RPC 2.0: LocalBoard as client (stdio / HTTP) and as a local server.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::storage::mcp::McpServerRecord;

const STDIO_TIMEOUT: Duration = Duration::from_secs(30);
pub const PROTOCOL: &str = "2024-11-05";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub read_only: bool,
}

pub fn is_write_like(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "write", "create", "update", "delete", "remove", "drop", "insert", "send", "post", "put", "patch", "exec",
        "execute", "apply", "mutate", "set_",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Read-only only when the server says so through tool annotations and the name gives no reason to
/// doubt it. A tool without annotations is treated as a write unless the user allow-lists it.
pub fn annotated_read_only(tool: &Value) -> bool {
    let annotations = tool.get("annotations");
    let hint = |key: &str| annotations.and_then(|value| value.get(key)).and_then(Value::as_bool);
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
    hint("readOnlyHint") == Some(true) && hint("destructiveHint") != Some(true) && !is_write_like(name)
}

pub fn audit_tool(workspace: &crate::state::Workspace, server: &str, tool: &str, read_only: bool, status: crate::storage::workspace_db::AuditStatus) {
    let _ = workspace.db().and_then(|db| {
        db.record_audit(&crate::storage::workspace_db::AuditEntry {
            kind: "mcp.tool",
            status,
            duration_ms: None,
            query: crate::storage::workspace_db::AuditQuery::default(),
            detail: json!({ "server": server, "tool": tool, "readOnly": read_only }),
        })
    });
}

pub fn wrap_untrusted(text: &str) -> String {
    use crate::engine::analyst::context::{DATA_CLOSE, DATA_OPEN};
    let flat: String = text.chars().filter(|ch| *ch != DATA_OPEN && *ch != DATA_CLOSE && !ch.is_control()).collect();
    format!("{DATA_OPEN}{flat}{DATA_CLOSE}")
}

pub async fn list_remote_tools(server: &McpServerRecord, offline: bool) -> AppResult<Vec<ToolInfo>> {
    let response = call_remote(server, "tools/list", json!({}), offline).await?;
    let tools = response.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?.to_owned();
            let description = tool.get("description").and_then(Value::as_str).unwrap_or("").to_owned();
            Some(ToolInfo { read_only: annotated_read_only(tool), name, description })
        })
        .collect())
}

pub async fn call_remote(server: &McpServerRecord, method: &str, params: Value, offline: bool) -> AppResult<Value> {
    match server.transport.as_str() {
        "http" => call_http(server, method, params, offline).await,
        "stdio" => call_stdio(server, method, params).await,
        other => Err(AppError::invalid(format!("Unknown MCP transport: {other}"))),
    }
}

async fn call_http(server: &McpServerRecord, method: &str, params: Value, offline: bool) -> AppResult<Value> {
    let url = server.url.as_deref().ok_or_else(|| AppError::invalid("This MCP server has no URL"))?;
    if !is_loopback(url) && offline {
        return Err(AppError::invalid("Offline mode is on. Remote MCP would leave this device."));
    }
    let request = JsonRpcRequest { jsonrpc: "2.0".into(), id: json!(1), method: method.to_owned(), params };
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(|error| AppError::Internal(error.to_string()))?;
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&request)
        .send()
        .await
        .map_err(|error| AppError::invalid(format!("MCP HTTP failed: {error}")))?;
    let body: JsonRpcResponse = response.json().await.map_err(|error| AppError::invalid(format!("MCP HTTP response: {error}")))?;
    rpc_result(body)
}

async fn call_stdio(server: &McpServerRecord, method: &str, params: Value) -> AppResult<Value> {
    let command = server.command.as_deref().ok_or_else(|| AppError::invalid("This MCP server has no command"))?;
    let workspace = std::env::current_dir().ok();
    tokio::task::spawn_blocking({
        let command = command.to_owned();
        let method = method.to_owned();
        move || stdio_roundtrip(&command, &method, params, workspace.as_deref())
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

fn stdio_roundtrip(command: &str, method: &str, params: Value, cwd: Option<&std::path::Path>) -> AppResult<Value> {
    let mut parts = shell_words(command);
    if parts.is_empty() {
        return Err(AppError::invalid("MCP command is empty"));
    }
    let program = parts.remove(0);
    let mut child = Command::new(program);
    child.args(parts).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    let mut child = child.spawn().map_err(|error| AppError::invalid(format!("Could not start MCP server: {error}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| AppError::Internal("MCP stdin missing".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| AppError::Internal("MCP stdout missing".into()))?;
    let (frames, incoming) = std::sync::mpsc::channel::<AppResult<Value>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        // Two replies are expected: initialize, then the call. Stop at the first failure or end of output.
        for _ in 0..2 {
            let frame = read_frame(&mut reader);
            let failed = frame.is_err();
            if frames.send(frame).is_err() || failed {
                break;
            }
        }
    });
    let wait = |child: &mut std::process::Child| -> AppResult<Value> {
        match incoming.recv_timeout(STDIO_TIMEOUT) {
            Ok(frame) => frame,
            Err(_) => {
                let _ = child.kill();
                Err(AppError::invalid("The MCP server did not answer in time"))
            }
        }
    };
    write_frame(&mut stdin, &JsonRpcRequest { jsonrpc: "2.0".into(), id: json!(1), method: "initialize".into(), params: json!({
        "protocolVersion": PROTOCOL,
        "capabilities": {},
        "clientInfo": { "name": "localboard", "version": env!("CARGO_PKG_VERSION") }
    }) })?;
    wait(&mut child)?;
    let _ = write_notification(&mut stdin, "notifications/initialized", json!({}));
    write_frame(&mut stdin, &JsonRpcRequest { jsonrpc: "2.0".into(), id: json!(2), method: method.to_owned(), params })?;
    let response = wait(&mut child);
    let _ = child.kill();
    let response = response?;
    let parsed: JsonRpcResponse = serde_json::from_value(response)?;
    rpc_result(parsed)
}

fn write_frame(writer: &mut dyn Write, request: &JsonRpcRequest) -> AppResult<()> {
    let body = serde_json::to_vec(request)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).map_err(|error| AppError::Internal(error.to_string()))?;
    writer.write_all(&body).map_err(|error| AppError::Internal(error.to_string()))?;
    writer.flush().map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

fn write_notification(writer: &mut dyn Write, method: &str, params: Value) -> AppResult<()> {
    let body = serde_json::to_vec(&json!({"jsonrpc":"2.0","method":method,"params":params}))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).map_err(|error| AppError::Internal(error.to_string()))?;
    writer.write_all(&body).map_err(|error| AppError::Internal(error.to_string()))?;
    writer.flush().map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

fn read_frame(reader: &mut BufReader<impl Read>) -> AppResult<Value> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|error| AppError::Internal(error.to_string()))?;
        if n == 0 {
            return Err(AppError::invalid("MCP server closed the connection"));
        }
        if line == "\r\n" || line == "\n" {
            if headers.is_empty() {
                continue;
            }
            break;
        }
        headers.push_str(&line);
    }
    let length = headers.lines().find_map(|line| {
        line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().parse::<usize>().ok())
    }).flatten();
    if let Some(length) = length {
        let mut buf = vec![0u8; length];
        reader.read_exact(&mut buf).map_err(|error| AppError::Internal(error.to_string()))?;
        return serde_json::from_slice(&buf).map_err(AppError::from);
    }
    // Newline-delimited JSON fallback: last header line may have been the body if the server skipped framing.
    let leftover = headers.trim();
    if leftover.starts_with('{') {
        return serde_json::from_str(leftover).map_err(AppError::from);
    }
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|error| AppError::Internal(error.to_string()))?;
    serde_json::from_str(line.trim()).map_err(AppError::from)
}

fn rpc_result(response: JsonRpcResponse) -> AppResult<Value> {
    if let Some(error) = response.error {
        return Err(AppError::invalid(format!("MCP: {}", error.message)));
    }
    Ok(response.result.unwrap_or(Value::Null))
}

pub fn is_loopback(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else { return false };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    matches!(parsed.host_str().map(str::to_ascii_lowercase).as_deref(), Some("127.0.0.1" | "localhost" | "[::1]"))
}

const MCP_TOKEN_REF: &str = "mcp.local-token";

/// The bearer token for LocalBoard's own /mcp endpoint. Created on first use and kept in the OS keychain,
/// where both the app and the `mcp` stdio proxy (a separate process) can read it.
pub fn local_token() -> AppResult<String> {
    if let Ok(token) = crate::storage::secrets::get(MCP_TOKEN_REF) {
        if token.len() >= 32 {
            return Ok(token);
        }
    }
    rotate_local_token()
}

pub fn rotate_local_token() -> AppResult<String> {
    let token = format!("lb_{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    crate::storage::secrets::save(MCP_TOKEN_REF, &token)?;
    Ok(token)
}

fn shell_words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in command.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Claude Desktop / Cursor stdio proxy: frames on stdin, POST to the local HTTP MCP endpoint.
pub fn stdio_proxy() {
    let mut stdin = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(120)).build();
    let Ok(client) = client else {
        eprintln!("localboard: could not start the MCP proxy");
        return;
    };
    loop {
        match read_frame(&mut stdin) {
            Ok(body) => {
                let token = std::env::var("LOCALBOARD_MCP_TOKEN").ok().or_else(|| local_token().ok()).unwrap_or_default();
                let posted = client.post("http://127.0.0.1:17890/mcp").bearer_auth(token).json(&body).send();
                let reply = match posted {
                    Ok(response) => response.json::<Value>().unwrap_or_else(|error| json!({
                        "jsonrpc": "2.0",
                        "id": body.get("id").cloned().unwrap_or(Value::Null),
                        "error": { "code": -32603, "message": error.to_string() }
                    })),
                    Err(error) => json!({
                        "jsonrpc": "2.0",
                        "id": body.get("id").cloned().unwrap_or(Value::Null),
                        "error": { "code": -32603, "message": format!("LocalBoard is not running on 127.0.0.1:17890. {error}") }
                    }),
                };
                if let Ok(bytes) = serde_json::to_vec(&reply) {
                    let _ = write!(stdout, "Content-Length: {}\r\n\r\n", bytes.len());
                    let _ = stdout.write_all(&bytes);
                    let _ = stdout.flush();
                }
            }
            Err(_) => break,
        }
    }
}

pub fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "localboard", "version": env!("CARGO_PKG_VERSION") }
    })
}

pub fn local_tools() -> Vec<Value> {
    [
        ("list_sources", "List connected data sources in the open workspace."),
        ("get_schema", "Get columns and types for a table. Argument: relation."),
        ("ask_analyst", "Ask the local analyst a question. Argument: question."),
        ("run_query", "Run a read-only guarded SQL query. Argument: sql."),
        ("list_dashboards", "List dashboards in the open workspace."),
        ("open_dashboard", "Open a dashboard by id. Argument: id."),
        ("get_insight", "Get the last computed insight for a dashboard tile. Arguments: dashboardId, tileId."),
        ("run_automation", "Run an existing automation by id. Argument: id."),
    ]
    .into_iter()
    .map(|(name, description)| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "additionalProperties": true }
        })
    })
    .collect()
}

pub type ToolHandler = Arc<dyn Fn(&str, Value) -> AppResult<Value> + Send + Sync>;

/// Builds a response for a fully-parsed request (HTTP path).
pub fn respond(request: JsonRpcRequest, handle: &ToolHandler) -> JsonRpcResponse {
    if request.method == "tools/call" {
        let name = request.params.get("name").and_then(Value::as_str).unwrap_or("");
        let arguments = request.params.get("arguments").cloned().unwrap_or(json!({}));
        return match handle(name, arguments) {
            Ok(result) => ok(request.id, json!({ "content": [{ "type": "text", "text": stringify(&result) }] })),
            Err(error) => fail(request.id, error.to_string()),
        };
    }
    match request.method.as_str() {
        "initialize" => ok(request.id, initialize_result()),
        "tools/list" => ok(request.id, json!({ "tools": local_tools() })),
        "ping" => ok(request.id, json!({})),
        other => fail(request.id, format!("Unknown method: {other}")),
    }
}

fn stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn ok(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
}

fn fail(id: Value, message: String) -> JsonRpcResponse {
    JsonRpcResponse { jsonrpc: "2.0".into(), id, result: None, error: Some(JsonRpcError { code: -32000, message }) }
}
