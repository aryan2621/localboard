use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tauri::State;

use crate::engine::mcp::{self, ToolInfo};
use crate::engine::query::{self, CancelToken};
use crate::error::{blocking, AppError, AppResult};
use crate::sql_guard::{self, GuardPolicy};
use crate::state::AppState;
use crate::engine::analyst::{AnalystEvent, EventSink, Models, Run};
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::storage::conversations::{title_from, MessageRole, MessageStatus, QueryStatus};
use crate::storage::mcp::{McpServerRecord, McpToolRule};
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};

/// Set once at startup so MCP tools that need an app handle (automation runs) can reach it.
pub static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

const ASK_TIMEOUT: Duration = Duration::from_secs(120);
const PREVIEW_ROWS: usize = 10;

pub struct HeadlessAnswer {
    pub text: String,
    pub sql: Option<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// Runs the analyst pipeline without a UI and waits for the answer, bounded by a timeout.
pub async fn ask_headless(state: &AppState, question: &str) -> AppResult<HeadlessAnswer> {
    let question = question.trim().to_owned();
    if question.is_empty() {
        return Err(AppError::invalid("question is required"));
    }
    if question.chars().count() > 20_000 {
        return Err(AppError::invalid("Questions are limited to 20000 characters"));
    }
    let workspace = state.workspace()?;
    crate::engine::mask::activate(workspace.policy());
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let models = Models::available(state.llm.clone(), routing);
    let offline = state.is_offline();
    let (conversation_id, user_id, assistant_id) =
        (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    let cancel = state.analyst_runs.begin(&assistant_id)?;
    let stored = {
        let (workspace, question) = (workspace.clone(), question.clone());
        let (conversation_id, user_id, assistant_id) = (conversation_id.clone(), user_id.clone(), assistant_id.clone());
        blocking(move || {
            let db = workspace.db()?;
            db.create_conversation(&conversation_id, &title_from(&question))?;
            db.insert_message(&user_id, &conversation_id, MessageRole::User, &question, MessageStatus::Done)?;
            db.insert_message(&assistant_id, &conversation_id, MessageRole::Assistant, "", MessageStatus::Pending)?;
            Ok(())
        })
        .await
    };
    if let Err(error) = stored {
        state.analyst_runs.finish(&assistant_id);
        return Err(error);
    }
    let sink: EventSink = Arc::new(|_: AnalystEvent| {});
    let mut run = Run::new(workspace.clone(), models, cancel.clone(), sink, conversation_id.clone(), assistant_id.clone(), question);
    run.set_offline(offline);
    let outcome = tokio::time::timeout(ASK_TIMEOUT, run.execute()).await;
    if outcome.is_err() {
        cancel.cancel();
    }
    state.analyst_runs.finish(&assistant_id);
    let text = outcome.map_err(|_| AppError::invalid("The analyst timed out"))?;
    let queries = blocking(move || workspace.db()?.conversation_queries(&conversation_id)).await?;
    let best = queries.into_iter().rev().find(|query| query.status == QueryStatus::Ok);
    let (sql, columns, rows) = match best {
        Some(query) => {
            let (columns, rows) = query
                .result
                .map(|result| (result.columns.into_iter().map(|column| column.name).collect(), result.rows.into_iter().take(PREVIEW_ROWS).collect()))
                .unwrap_or_default();
            (Some(query.sql), columns, rows)
        }
        None => (None, Vec::new(), Vec::new()),
    };
    Ok(HeadlessAnswer { text, sql, columns, rows })
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tauri::async_runtime::block_on(future))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDraft {
    pub id: Option<String>,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRuleDraft {
    pub agent_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub allow: bool,
}

#[tauri::command]
pub async fn mcp_list_servers(state: State<'_, AppState>) -> AppResult<Vec<McpServerRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_mcp_servers()).await
}

#[tauri::command]
pub async fn mcp_save_server(state: State<'_, AppState>, draft: McpServerDraft) -> AppResult<McpServerRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || {
        let name = draft.name.trim();
        if name.is_empty() {
            return Err(AppError::invalid("Name the MCP server"));
        }
        if draft.transport != "stdio" && draft.transport != "http" {
            return Err(AppError::invalid("Transport must be stdio or http"));
        }
        if draft.transport == "stdio" && draft.command.as_deref().unwrap_or("").trim().is_empty() {
            return Err(AppError::invalid("A stdio server needs a command"));
        }
        if draft.transport == "http" && draft.url.as_deref().unwrap_or("").trim().is_empty() {
            return Err(AppError::invalid("An HTTP server needs a URL"));
        }
        let record = McpServerRecord {
            id: draft.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name: name.into(),
            transport: draft.transport,
            command: draft.command.filter(|text| !text.trim().is_empty()),
            url: draft.url.filter(|text| !text.trim().is_empty()),
            enabled: draft.enabled,
        };
        workspace.db()?.upsert_mcp_server(&record)
    })
    .await
}

#[tauri::command]
pub async fn mcp_delete_server(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_mcp_server(&id)).await
}

#[tauri::command]
pub async fn mcp_list_tools(state: State<'_, AppState>, server_id: String) -> AppResult<Vec<ToolInfo>> {
    let workspace = state.workspace()?;
    let offline = state.is_offline();
    let server = blocking({
        let workspace = workspace.clone();
        let server_id = server_id.clone();
        move || workspace.db()?.mcp_server(&server_id)
    })
    .await?;
    if !server.enabled {
        return Err(AppError::invalid("This MCP server is disabled"));
    }
    mcp::list_remote_tools(&server, offline).await
}

#[tauri::command]
pub async fn mcp_list_rules(state: State<'_, AppState>, agent_id: Option<String>) -> AppResult<Vec<McpToolRule>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_mcp_rules(agent_id.as_deref())).await
}

#[tauri::command]
pub async fn mcp_set_rule(state: State<'_, AppState>, draft: McpRuleDraft) -> AppResult<()> {
    let workspace = state.workspace()?;
    blocking(move || {
        workspace.db()?.set_mcp_rule(&McpToolRule {
            agent_id: draft.agent_id,
            server_id: draft.server_id,
            tool_name: draft.tool_name,
            allow: draft.allow,
        })
    })
    .await
}

#[tauri::command]
pub async fn mcp_delete_rule(state: State<'_, AppState>, agent_id: String, server_id: String, tool_name: String) -> AppResult<()> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_mcp_rule(&agent_id, &server_id, &tool_name)).await
}

fn audit_tool(workspace: &crate::state::Workspace, server: &str, tool: &str, read_only: bool, status: AuditStatus) {
    crate::engine::mcp::audit_tool(workspace, server, tool, read_only, status);
}

/// Executes a LocalBoard-hosted MCP tool. Payloads are never written to audit SQL fields.
pub fn run_local_tool(state: &AppState, name: &str, arguments: Value) -> AppResult<Value> {
    let workspace = state.workspace()?;
    crate::engine::mask::activate(workspace.policy());
    let result = match name {
        "list_sources" => {
            let connections = workspace.db()?.list_connections()?;
            Ok(json!(connections
                .into_iter()
                .map(|item| json!({ "id": item.id, "name": item.name, "kind": item.kind.as_str() }))
                .collect::<Vec<_>>()))
        }
        "get_schema" => {
            let relation = arguments.get("relation").and_then(Value::as_str).ok_or_else(|| AppError::invalid("relation is required"))?;
            let tables = workspace.db()?.list_tables()?;
            let table = tables.into_iter().find(|table| table.relation == relation).ok_or_else(|| AppError::not_found("Table not found"))?;
            let columns = workspace.db()?.columns(&table.id)?;
            Ok(json!({
                "relation": table.relation,
                "columns": columns.into_iter().map(|column| json!({ "name": column.name, "type": column.data_type })).collect::<Vec<_>>()
            }))
        }
        "run_query" => {
            let sql = arguments.get("sql").and_then(Value::as_str).ok_or_else(|| AppError::invalid("sql is required"))?;
            let relations = workspace.db()?.list_tables()?.into_iter().map(|table| table.relation).collect::<Vec<_>>();
            let policy = GuardPolicy::new(relations);
            let validated = sql_guard::validate(sql, &policy).map_err(|error| AppError::invalid(error.to_string()))?;
            crate::engine::mask::authorize_query(&validated)?;
            let result = query::run(&workspace.query_connection()?, &validated, &Default::default(), &CancelToken::default())
                .map_err(|error| AppError::invalid(error.to_string()))?;
            let _ = workspace.db()?.record_audit_lineage(
                &AuditEntry {
                    kind: "mcp.tool",
                    status: AuditStatus::Ok,
                    duration_ms: None,
                    query: AuditQuery { sql: Some(sql), rows: Some(result.rows.len() as i64), ..AuditQuery::default() },
                    detail: json!({ "tool": "run_query", "readOnly": true }),
                },
                &validated.relations,
            );
            Ok(json!({ "columns": result.columns, "rows": result.rows, "truncated": result.truncated }))
        }
        "list_dashboards" => {
            let boards = workspace.db()?.list_dashboards()?;
            Ok(json!(boards))
        }
        "open_dashboard" => {
            let id = arguments.get("id").and_then(Value::as_str).ok_or_else(|| AppError::invalid("id is required"))?;
            let board = workspace.db()?.dashboard(id)?;
            // Dashboards open inside the app; `/d/` on the local server serves datasites.
            let url = Some(format!("localboard://dashboard/{id}"));
            Ok(json!({ "dashboard": board, "url": url }))
        }
        "get_insight" => {
            let dashboard_id = arguments.get("dashboardId").and_then(Value::as_str).ok_or_else(|| AppError::invalid("dashboardId is required"))?;
            let tile_id = arguments.get("tileId").and_then(Value::as_str);
            let board = workspace.db()?.dashboard(dashboard_id)?;
            let insight = board.tiles.into_iter().find(|tile| tile_id.is_none_or(|id| tile.id == id)).and_then(|tile| tile.insight);
            Ok(json!({ "insight": insight }))
        }
        "run_automation" => {
            let id = arguments.get("id").and_then(Value::as_str).ok_or_else(|| AppError::invalid("id is required"))?.to_owned();
            let app = APP_HANDLE.get().ok_or_else(|| AppError::invalid("LocalBoard is still starting"))?.clone();
            block_on(async {
                crate::commands::automations::run_one(&app, state, &id, "mcp").await?;
                let _ = tauri::Emitter::emit(&app, crate::commands::automations::AUTOMATION_EVENT, json!({ "id": id }));
                let runs = {
                    let workspace = workspace.clone();
                    let id = id.clone();
                    blocking(move || workspace.db()?.automation_runs(&id)).await?
                };
                let latest = runs.into_iter().next().ok_or_else(|| AppError::not_found("The run was not recorded"))?;
                Ok(json!({
                    "runId": latest.id,
                    "status": latest.status,
                    "summary": latest.summary.as_deref().map(mcp::wrap_untrusted),
                    "steps": latest.steps.iter().map(|step| json!({
                        "kind": step.kind,
                        "status": step.status,
                        "summary": step.summary.as_deref().map(mcp::wrap_untrusted),
                        "facts": mcp::wrap_untrusted(&step.facts.as_ref().map(Value::to_string).unwrap_or_default()),
                    })).collect::<Vec<_>>(),
                }))
            })
        }
        "ask_analyst" => {
            let question = arguments.get("question").and_then(Value::as_str).unwrap_or("").trim().to_owned();
            if question.is_empty() {
                return Err(AppError::invalid("question is required"));
            }
            let answer = block_on(ask_headless(state, &question))?;
            let rows: Vec<String> = answer.rows.iter().map(|row| mcp::wrap_untrusted(&serde_json::to_string(row).unwrap_or_default())).collect();
            Ok(json!({
                "answer": mcp::wrap_untrusted(&answer.text),
                "sql": answer.sql.as_deref().map(mcp::wrap_untrusted),
                "columns": answer.columns,
                "rowsPreview": rows,
            }))
        }
        "list_tools" => Ok(json!(mcp::local_tools())),
        other => Err(AppError::invalid(format!("Unknown tool: {other}"))),
    };
    if name != "run_query" {
        audit_tool(&workspace, "localboard", name, true, if result.is_ok() { AuditStatus::Ok } else { AuditStatus::Error });
    }
    result
}
