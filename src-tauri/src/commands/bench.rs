//! Query Bench: guarded SQL, bound parameters, history, exports, and model help.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use duckdb::types::Value as DuckValue;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::engine::analyst::{context, prompts};
use crate::engine::query::{self, bind_params, duck_placeholders, elapsed_ms, CancelToken, QueryResult};
use crate::error::{blocking, AppError, AppResult};
use crate::llm::provider::{ChatMessage, ChatRequest, LlmError, LlmProvider, ResponseFormat, Role, Task};
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::sql_guard::{self, GuardPolicy};
use crate::state::AppState;
use crate::storage::bench::{HistoryEntry, SavedQuery, SchemaTable};
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};

const MAX_SQL: usize = 20_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchOutcome {
    pub sql: String,
    pub result: QueryResult,
    pub duration_ms: i64,
}

#[derive(Serialize)]
pub struct SqlText {
    pub sql: String,
}

#[derive(Serialize)]
pub struct Explanation {
    pub text: String,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn params_of(params: Value) -> AppResult<serde_json::Map<String, Value>> {
    match params {
        Value::Null => Ok(serde_json::Map::new()),
        Value::Object(map) => Ok(map),
        _ => Err(AppError::invalid("Parameters are a set of names and values.")),
    }
}

async fn prepare(state: &AppState, sql: &str) -> AppResult<(crate::sql_guard::ValidatedQuery, HashMap<String, DuckValue>, serde_json::Map<String, Value>)> {
    let sql = sql.trim().to_owned();
    if sql.chars().count() > MAX_SQL {
        return Err(AppError::invalid(format!("Queries are limited to {MAX_SQL} characters")));
    }
    let workspace = state.workspace()?;
    let validated = blocking({
        let workspace = workspace.clone();
        let sql = sql.clone();
        move || {
            let relations = workspace.db()?.list_tables()?.into_iter().map(|table| table.relation);
            sql_guard::validate(&sql, &GuardPolicy::new(relations)).map_err(|error| AppError::invalid(error.to_string()))
        }
    })
    .await?;
    Ok((validated, HashMap::new(), serde_json::Map::new()))
}

async fn prepare_bound(
    state: &AppState,
    sql: &str,
    params: Value,
) -> AppResult<(crate::sql_guard::ValidatedQuery, HashMap<String, DuckValue>, serde_json::Map<String, Value>)> {
    let raw = params_of(params)?;
    let (validated, _, _) = prepare(state, sql).await?;
    let bound = bind_params(&validated.parameters, &raw).map_err(AppError::invalid)?;
    Ok((validated, bound, raw))
}

fn audit_run(
    state: &AppState,
    sql: &str,
    params: &serde_json::Map<String, Value>,
    status: AuditStatus,
    error: Option<&str>,
    rows: Option<i64>,
    duration: Option<i64>,
    relations: &[String],
) {
    let workspace = match state.workspace() {
        Ok(workspace) => workspace,
        Err(error) => {
            tracing::warn!("could not write audit entry bench.run: {error}");
            return;
        }
    };
    let entry = AuditEntry {
        kind: "bench.run",
        status,
        duration_ms: duration,
        query: AuditQuery { question: None, sql: Some(sql), rows },
        detail: json!({ "params": params, "error": error }),
    };
    match workspace.db() {
        Ok(db) => db.try_record_audit_lineage(&entry, relations),
        Err(error) => tracing::error!("could not write audit entry bench.run: {error}"),
    };
}

fn record(state: &AppState, sql: &str, params: &serde_json::Map<String, Value>, status: &str, error: Option<&str>, rows: Option<i64>, duration: Option<i64>) {
    let workspace = match state.workspace() {
        Ok(workspace) => workspace,
        Err(error) => {
            tracing::warn!("could not record query history: {error}");
            return;
        }
    };
    let (sql, params, status, error, id) = (sql.to_owned(), serde_json::to_string(params).unwrap_or_else(|_| "{}".into()), status.to_owned(), error.map(str::to_owned), new_id());
    tauri::async_runtime::spawn(async move {
        let saved = blocking(move || {
            workspace.db()?.insert_history(&id, &sql, &params, &status, error.as_deref(), rows, duration)
        })
        .await;
        if let Err(failure) = saved {
            tracing::warn!("could not record query history: {failure}");
        }
    });
}

#[tauri::command]
pub async fn bench_schema(state: State<'_, AppState>) -> AppResult<Vec<SchemaTable>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.schema_index()).await
}

#[tauri::command]
pub async fn bench_format(state: State<'_, AppState>, sql: String) -> AppResult<SqlText> {
    let (validated, _, _) = prepare(&state, &sql).await?;
    Ok(SqlText { sql: validated.sql })
}

#[tauri::command]
pub async fn bench_run(state: State<'_, AppState>, sql: String, params: Value) -> AppResult<BenchOutcome> {
    let started = Instant::now();
    let (validated, bound, raw) = match prepare_bound(&state, &sql, params).await {
        Ok(ready) => ready,
        Err(error) => {
            record(&state, &sql, &serde_json::Map::new(), "denied", Some(&error.to_string()), None, None);
            audit_run(&state, &sql, &serde_json::Map::new(), AuditStatus::Denied, Some(&error.to_string()), None, Some(elapsed_ms(started)), &[]);
            return Err(error);
        }
    };
    let rendered = validated.sql.clone();
    let relations = validated.relations.clone();
    let workspace = state.workspace()?;
    let ran = blocking(move || {
        query::run(&workspace.query_connection_unmasked()?, &validated, &bound, &CancelToken::default()).map_err(|error| AppError::invalid(error.to_string()))
    })
    .await;
    let duration = elapsed_ms(started);
    match ran {
        Ok(result) => {
            record(&state, &rendered, &raw, "ok", None, Some(result.rows.len() as i64), Some(duration));
            audit_run(&state, &rendered, &raw, AuditStatus::Ok, None, Some(result.rows.len() as i64), Some(duration), &relations);
            Ok(BenchOutcome { sql: rendered, result, duration_ms: duration })
        }
        Err(error) => {
            record(&state, &rendered, &raw, "error", Some(&error.to_string()), None, Some(duration));
            audit_run(&state, &rendered, &raw, AuditStatus::Error, Some(&error.to_string()), None, Some(duration), &relations);
            Err(AppError::invalid(error.to_string()))
        }
    }
}

#[tauri::command]
pub async fn bench_explain(state: State<'_, AppState>, sql: String, params: Value) -> AppResult<QueryResult> {
    let (mut validated, bound, _) = prepare_bound(&state, &sql, params).await?;
    if !validated.sql.trim_start().to_ascii_uppercase().starts_with("EXPLAIN") {
        validated.sql = format!("EXPLAIN {}", validated.sql);
        validated.has_limit = true;
    }
    let workspace = state.workspace()?;
    blocking(move || {
        query::run(&workspace.query_connection_unmasked()?, &validated, &bound, &CancelToken::default()).map_err(|error| AppError::invalid(error.to_string()))
    })
    .await
}

#[tauri::command]
pub async fn bench_fix(state: State<'_, AppState>, sql: String, error: String) -> AppResult<SqlText> {
    let (validated, _, _) = prepare(&state, &sql).await?;
    let fixed = rewrite(&state, &validated.sql, &error, true).await?;
    let (checked, _, _) = prepare(&state, &fixed).await?;
    Ok(SqlText { sql: checked.sql })
}

#[tauri::command]
pub async fn bench_optimize(state: State<'_, AppState>, sql: String) -> AppResult<SqlText> {
    let (validated, _, _) = prepare(&state, &sql).await?;
    let optimized = rewrite(&state, &validated.sql, "", false).await?;
    let (checked, _, _) = prepare(&state, &optimized).await?;
    Ok(SqlText { sql: checked.sql })
}

#[tauri::command]
pub async fn bench_describe(state: State<'_, AppState>, sql: String) -> AppResult<Explanation> {
    let (validated, _, _) = prepare(&state, &sql).await?;
    let provider = sql_provider(&state).await?;
    let response = provider
        .chat(
            ChatRequest {
                messages: vec![
                    ChatMessage {
                        role: Role::System,
                        content: "Explain this SQL in a few sentences. Do not invent numbers or results.".into(),
                    },
                    ChatMessage { role: Role::User, content: validated.sql },
                ],
                temperature: prompts::SQL_TEMPERATURE,
                max_tokens: prompts::SQL_MAX_TOKENS,
                format: ResponseFormat::Text,
            },
            None,
        )
        .await?;
    Ok(Explanation { text: response.content.trim().to_owned() })
}

#[tauri::command]
pub async fn bench_export(state: State<'_, AppState>, sql: String, params: Value, format: String, path: String) -> AppResult<()> {
    let (validated, bound, _) = prepare_bound(&state, &sql, params).await?;
    let workspace = state.workspace()?;
    let target = PathBuf::from(&path);
    let kind = format.to_ascii_lowercase();
    blocking(move || {
        let conn = workspace.query_connection_unmasked()?;
        let result = query::run(&conn, &validated, &bound, &CancelToken::default()).map_err(|error| AppError::invalid(error.to_string()))?;
        match kind.as_str() {
            "csv" => write_csv(&target, &result),
            "json" => std::fs::write(&target, serde_json::to_vec_pretty(&result)?).map_err(AppError::from),
            "xlsx" => write_xlsx(&target, &result),
            "parquet" => write_parquet(&conn, &validated, &bound, &target),
            _ => Err(AppError::invalid("Export as csv, json, xlsx or parquet.")),
        }
    })
    .await
}

#[tauri::command]
pub async fn bench_history(state: State<'_, AppState>) -> AppResult<Vec<HistoryEntry>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_history()).await
}

#[tauri::command]
pub async fn bench_saved(state: State<'_, AppState>) -> AppResult<Vec<SavedQuery>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_saved_queries()).await
}

#[tauri::command]
pub async fn bench_save_query(state: State<'_, AppState>, id: Option<String>, name: String, sql: String) -> AppResult<SavedQuery> {
    state.ensure_unlocked()?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(AppError::invalid("Name the query"));
    }
    let (validated, _, _) = prepare(&state, &sql).await?;
    let workspace = state.workspace()?;
    let id = id.filter(|value| !value.is_empty()).unwrap_or_else(new_id);
    blocking(move || {
        let db = workspace.db()?;
        let saved = db.save_query(&id, &name, &validated.sql)?;
        db.replace_query_relations("saved", &saved.id, &validated.relations)?;
        Ok(saved)
    })
    .await
}

#[tauri::command]
pub async fn bench_delete_query(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        db.clear_query_relations("saved", &id)?;
        db.delete_saved_query(&id)
    })
    .await
}

async fn sql_provider(state: &AppState) -> AppResult<crate::llm::openai_compat::OpenAiCompatProvider> {
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let spec = state.llm.resolve(&routing, Task::Sql).ok_or(LlmError::NoModel)?;
    Ok(state.llm.provider(spec.id).await?)
}

async fn rewrite(state: &AppState, sql: &str, error: &str, fixing: bool) -> AppResult<String> {
    let workspace = state.workspace()?;
    let sql_for_schema = sql.to_owned();
    let schema = blocking(move || context::build(&workspace, &sql_for_schema, &[])).await?;
    let provider = sql_provider(state).await?;
    let system = if fixing {
        prompts::repair_system(&schema.prompt, "")
    } else {
        prompts::optimize_system(&schema.prompt)
    };
    let user = if fixing { prompts::repair_user("Query Bench", sql, error) } else { sql.to_owned() };
    let response = provider
        .chat(
            ChatRequest {
                messages: vec![ChatMessage { role: Role::System, content: system }, ChatMessage { role: Role::User, content: user }],
                temperature: prompts::SQL_TEMPERATURE,
                max_tokens: prompts::SQL_MAX_TOKENS,
                format: ResponseFormat::JsonSchema { name: "repair".into(), schema: prompts::repair_schema() },
            },
            None,
        )
        .await?;
    let parsed: prompts::Repaired = serde_json::from_str(response.content.trim())
        .map_err(|_| AppError::invalid("The model returned an unreadable query. Try again."))?;
    if parsed.sql.trim().is_empty() {
        return Err(AppError::invalid("The model did not return a query."));
    }
    Ok(parsed.sql)
}

fn write_csv(path: &PathBuf, result: &QueryResult) -> AppResult<()> {
    let mut out = String::new();
    out.push_str(&result.columns.iter().map(|column| csv_cell(&column.name)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for row in &result.rows {
        let cells = row.iter().map(|value| csv_cell(&cell_text(value))).collect::<Vec<_>>().join(",");
        out.push_str(&cells);
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) { format!("\"{}\"", value.replace('"', "\"\"")) } else { value.to_owned() }
}

fn cell_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn write_xlsx(path: &PathBuf, result: &QueryResult) -> AppResult<()> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    for (column, header) in result.columns.iter().enumerate() {
        sheet.write_string(0, column as u16, &header.name).map_err(|error| AppError::Internal(error.to_string()))?;
    }
    for (row_index, row) in result.rows.iter().enumerate() {
        for (column, value) in row.iter().enumerate() {
            let row_index = (row_index + 1) as u32;
            let column = column as u16;
            let written = match value {
                Value::Null => Ok(()),
                Value::Bool(flag) => sheet.write_boolean(row_index, column, *flag).map(|_| ()),
                Value::Number(number) => sheet.write_number(row_index, column, number.as_f64().unwrap_or(0.0)).map(|_| ()),
                other => sheet.write_string(row_index, column, &cell_text(other)).map(|_| ()),
            };
            written.map_err(|error| AppError::Internal(error.to_string()))?;
        }
    }
    workbook.save(path).map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

fn write_parquet(
    conn: &duckdb::Connection,
    query: &crate::sql_guard::ValidatedQuery,
    params: &HashMap<String, DuckValue>,
    path: &PathBuf,
) -> AppResult<()> {
    let rendered = duck_placeholders(&query.sql);
    let inner = if query.has_limit { rendered } else { format!("{rendered} LIMIT {}", query::MAX_ROWS) };
    let escaped = path.to_string_lossy().replace('\'', "''");
    let sql = format!("COPY ({inner}) TO '{escaped}' (FORMAT PARQUET)");
    let mut statement = conn.prepare(&sql)?;
    statement.execute(params)?;
    Ok(())
}
