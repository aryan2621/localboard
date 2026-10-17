//! Prompt-to-dashboard, guarded tile queries, insights and layout.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::engine::analyst::{chart, context, facts, prompts, StepStatus};
use crate::engine::query::{self, bind_params, elapsed_ms, CancelToken, QueryResult};
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};
use crate::error::{blocking, AppError, AppResult};
use crate::llm::provider::{ChatMessage, ChatRequest, LlmError, LlmProvider, ResponseFormat, Role, Task};
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::retrieval;
use crate::sql_guard::{self, GuardPolicy};
use crate::state::{AppState, Workspace};
use crate::storage::dashboards::{DashboardRecord, DashboardSummary, DashboardVersion, FilterDef, FolderRecord, TileLayout, TileRecord};

pub const DASHBOARD_EVENT: &str = "dashboard://refreshed";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTile {
    #[serde(flatten)]
    pub tile: TileRecord,
    pub result: Option<QueryResult>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveDashboard {
    pub dashboard: DashboardRecord,
    pub tiles: Vec<LiveTile>,
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Proposed {
    name: String,
    tiles: Vec<ProposedTile>,
    filters: Vec<FilterDef>,
}

#[derive(Deserialize)]
struct ProposedTile {
    title: String,
    sql: String,
    chart: String,
}

#[derive(Deserialize)]
pub struct LayoutPatch {
    pub id: String,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn params_of(params: Value) -> AppResult<serde_json::Map<String, Value>> {
    match params {
        Value::Null => Ok(serde_json::Map::new()),
        Value::Object(map) => Ok(map),
        _ => Err(AppError::invalid("Filters are a set of names and values.")),
    }
}

async fn policy(workspace: &std::sync::Arc<Workspace>) -> AppResult<GuardPolicy> {
    let workspace = workspace.clone();
    blocking(move || Ok(GuardPolicy::new(workspace.db()?.list_tables()?.into_iter().map(|table| table.relation)))).await
}

/// Cross-filters travel in the filter map as `__xf.<column>` entries.
const CROSS_PREFIX: &str = "__xf.";

fn run_sql(workspace: &Workspace, sql: &str, policy: &GuardPolicy, raw: &serde_json::Map<String, Value>) -> Result<(String, QueryResult, Vec<String>), String> {
    let validated = sql_guard::validate(sql, policy).map_err(|error| error.to_string())?;
    let bound = bind_params(&validated.parameters, raw).map_err(|error| error)?;
    let conn = workspace.query_connection().map_err(|error| error.to_string())?;
    let crosses: Vec<(&str, &Value)> = raw.iter().filter_map(|(key, value)| key.strip_prefix(CROSS_PREFIX).map(|column| (column, value))).collect();
    if !crosses.is_empty() {
        // Wrap the tile and filter on each dimension through bound parameters; a tile without the
        // column fails to bind and is shown unfiltered.
        let mut wrapped_raw = raw.clone();
        let mut clauses = Vec::new();
        for (i, (column, value)) in crosses.iter().enumerate() {
            clauses.push(format!("CAST(\"{}\" AS VARCHAR) = CAST(:xf{i} AS VARCHAR)", column.replace('"', "\"\"")));
            wrapped_raw.insert(format!("xf{i}"), (*value).clone());
        }
        let wrapped = format!("SELECT * FROM ({}) AS xf_src WHERE {}", validated.sql.trim().trim_end_matches(';'), clauses.join(" AND "));
        if let Ok(checked) = sql_guard::validate(&wrapped, policy) {
            if let Ok(wrapped_bound) = bind_params(&checked.parameters, &wrapped_raw) {
                if let Ok(result) = query::run(&conn, &checked, &wrapped_bound, &CancelToken::default()) {
                    return Ok((validated.sql, result, validated.relations));
                }
            }
        }
    }
    let result = query::run(&conn, &validated, &bound, &CancelToken::default()).map_err(|error| error.to_string())?;
    Ok((validated.sql, result, validated.relations))
}

/// Wide-open values for a freshly proposed dashboard's filters, so its tiles can run before anyone picks a range.
/// Values used while a filter is unset: dates get an open range, anything else is NULL ("all").
fn default_params(filters: &[FilterDef]) -> serde_json::Map<String, Value> {
    let mut params = serde_json::Map::new();
    for filter in filters {
        if filter.kind == "date" {
            let name = filter.name.to_ascii_lowercase();
            let upper = name.ends_with("_to") || name.ends_with("_end") || name.ends_with("_until");
            params.insert(filter.name.clone(), Value::String(if upper { "2999-12-31" } else { "1900-01-01" }.to_owned()));
        } else {
            params.insert(filter.name.clone(), Value::Null);
        }
    }
    params
}

fn is_ident(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Names written as `:name` in the SQL, skipping `::type` casts and quoted text.
fn sql_params(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut names = Vec::new();
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' {
            quoted = !quoted;
        } else if !quoted && ch == ':' && chars.get(i + 1).is_some_and(|next| next.is_ascii_alphabetic() || *next == '_') && (i == 0 || chars[i - 1] != ':') {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && is_ident(chars[end]) {
                end += 1;
            }
            let name: String = chars[start..end].iter().collect();
            if !names.contains(&name) {
                names.push(name);
            }
            i = end;
            continue;
        }
        i += 1;
    }
    names
}

/// Turns `column = :name` into `(:name IS NULL OR column = :name)`, so an unset filter shows every row.
fn optional_equals(sql: &str, name: &str) -> String {
    let needle = format!(":{name}");
    let mut out = sql.to_owned();
    let mut search_end = out.len();
    while let Some(at) = out[..search_end].rfind(&needle) {
        search_end = at;
        let after = at + needle.len();
        if out[after..].chars().next().is_some_and(is_ident) {
            continue;
        }
        let before = out[..at].trim_end();
        let Some(before_eq) = before.strip_suffix('=') else { continue };
        if before_eq.ends_with(['<', '>', '!']) {
            continue;
        }
        let column_end = before_eq.trim_end().len();
        let column_start = before_eq[..column_end]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| is_ident(*ch) || *ch == '.' || *ch == '"')
            .last()
            .map(|(index, _)| index)
            .unwrap_or(column_end);
        if column_start == column_end {
            continue;
        }
        if out[..column_start].trim_end().to_ascii_uppercase().ends_with("IS NULL OR") {
            continue;
        }
        let column = out[column_start..column_end].to_owned();
        out.replace_range(column_start..after, &format!("({needle} IS NULL OR {column} = {needle})"));
        search_end = column_start;
    }
    out
}

fn progress(app: &AppHandle, id: &str, label: impl Into<String>, status: StepStatus, detail: Option<String>) {
    super::progress(Some(app), id, label, status, detail);
}

fn next_layout(existing: &[TileRecord], kind: chart::ChartKind) -> TileLayout {
    let (w, h) = match kind {
        chart::ChartKind::Kpi => (3, 3),
        _ => (6, 5),
    };
    let mut y = existing.iter().map(|tile| tile.layout.y + tile.layout.h).max().unwrap_or(0);
    let mut x = 0;
    if let Some(last) = existing.last() {
        if last.layout.x + last.layout.w + w <= 12 && last.layout.y == y.saturating_sub(last.layout.h) {
            x = last.layout.x + last.layout.w;
            y = last.layout.y;
        }
    }
    TileLayout { x, y, w, h }
}

async fn insight_for(state: &AppState, title: &str, result: &QueryResult, previous: Option<&str>) -> AppResult<(String, String)> {
    let computed = facts::compute(result);
    let facts_json = serde_json::to_value(&computed)?;
    let hash = retrieval::content_hash(&facts_json.to_string());
    if previous.is_some_and(|existing| existing == hash) {
        return Ok((String::new(), hash));
    }
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let spec = match state.llm.resolve(&routing, Task::Narrative) {
        Some(spec) => spec,
        None => return Ok((String::new(), hash)),
    };
    let provider = state.llm.provider(spec.id).await?;
    let response = provider
        .chat(
            ChatRequest {
                messages: vec![
                    ChatMessage { role: Role::System, content: prompts::INSIGHT_SYSTEM.to_owned() },
                    ChatMessage { role: Role::User, content: prompts::narrative_user(title, &facts_json) },
                ],
                temperature: prompts::NARRATIVE_TEMPERATURE,
                max_tokens: 120,
                format: ResponseFormat::Text,
            },
            None,
        )
        .await?;
    Ok((response.content.trim().to_owned(), hash))
}

pub(crate) async fn live_tiles(
    state: &AppState,
    workspace: &std::sync::Arc<Workspace>,
    dashboard_id: &str,
    tiles: Vec<TileRecord>,
    raw: &serde_json::Map<String, Value>,
) -> AppResult<Vec<LiveTile>> {
    let policy = policy(workspace).await?;
    let mut live = Vec::with_capacity(tiles.len());
    for mut tile in tiles {
        if tile.chart.kind == chart::ChartKind::Text {
            live.push(LiveTile { tile, result: None, error: None });
            continue;
        }
        let ran = {
            let workspace = workspace.clone();
            let sql = tile.sql.clone();
            let policy = policy.clone();
            let raw = raw.clone();
            let dashboard_id = dashboard_id.to_owned();
            let tile_id = tile.id.clone();
            let title = tile.title.clone();
            blocking(move || {
                let started = Instant::now();
                let outcome = run_sql(&workspace, &sql, &policy, &raw);
                let duration = elapsed_ms(started);
                let (status, sql_text, rows, error, relations) = match &outcome {
                    Ok((sql_text, result, relations)) => {
                        (AuditStatus::Ok, sql_text.clone(), Some(result.rows.len() as i64), None, relations.clone())
                    }
                    Err(message) => {
                        let denied = sql_guard::validate(&sql, &policy).is_err();
                        (if denied { AuditStatus::Denied } else { AuditStatus::Error }, sql.clone(), None, Some(message.clone()), Vec::new())
                    }
                };
                if let Ok(db) = workspace.db() {
                    db.try_record_audit_lineage(
                        &AuditEntry {
                            kind: "dashboard.tile",
                            status,
                            duration_ms: Some(duration),
                            query: AuditQuery { question: Some(&title), sql: Some(&sql_text), rows },
                            detail: json!({ "dashboard": dashboard_id, "tile": tile_id, "error": error }),
                        },
                        &relations,
                    );
                    if !relations.is_empty() {
                        let _ = db.replace_query_relations("tile", &tile_id, &relations);
                    }
                }
                outcome.map(|(sql, result, _)| (sql, result)).map_err(AppError::invalid)
            })
            .await
        };
        match ran {
            Ok((sql, result)) => {
                tile.sql = sql;
                tile.chart.query_id = tile.id.clone();
                let previous = tile.facts_hash.clone();
                match insight_for(state, &tile.title, &result, previous.as_deref()).await {
                    Ok((text, hash)) if !text.is_empty() => {
                        tile.insight = Some(text.clone());
                        tile.facts_hash = Some(hash.clone());
                        let workspace = workspace.clone();
                        let id = tile.id.clone();
                        if let Err(error) = blocking(move || workspace.db()?.set_tile_insight(&id, &text, &hash)).await {
                            tracing::warn!("could not save a tile insight: {error}");
                        }
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!("insight for tile {} failed: {error}", tile.id),
                }
                live.push(LiveTile { tile, result: Some(result), error: None });
            }
            Err(error) => live.push(LiveTile { tile, result: None, error: Some(error.to_string()) }),
        }
    }
    Ok(live)
}

#[tauri::command]
pub async fn dashboard_folders(state: State<'_, AppState>) -> AppResult<Vec<FolderRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_folders()).await
}

#[tauri::command]
pub async fn dashboard_create_folder(state: State<'_, AppState>, name: String) -> AppResult<FolderRecord> {
    state.ensure_unlocked()?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(AppError::invalid("Name the folder"));
    }
    let workspace = state.workspace()?;
    let id = new_id();
    blocking(move || workspace.db()?.insert_folder(&id, &name)).await
}

#[tauri::command]
pub async fn dashboard_list(state: State<'_, AppState>) -> AppResult<Vec<DashboardSummary>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_dashboards()).await
}

#[tauri::command]
pub async fn dashboard_get(state: State<'_, AppState>, id: String, params: Value) -> AppResult<LiveDashboard> {
    let mut raw = params_of(params)?;
    let workspace = state.workspace()?;
    let dashboard = blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || workspace.db()?.dashboard(&id)
    })
    .await?;
    // A date filter nobody has set yet (or that was cleared) falls back to an open range.
    for (name, value) in default_params(&dashboard.filters) {
        if raw.get(&name).map_or(true, |current| current.as_str().is_some_and(|text| text.trim().is_empty()) || current.is_null()) {
            raw.insert(name, value);
        }
    }
    let params_json = serde_json::to_string(&raw)?;
    blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || workspace.db()?.set_last_params(&id, &params_json)
    })
    .await?;
    let tiles = live_tiles(&state, &workspace, &id, dashboard.tiles.clone(), &raw).await?;
    Ok(LiveDashboard { dashboard, tiles, warnings: Vec::new() })
}

#[tauri::command]
pub async fn dashboard_generate(app: AppHandle, state: State<'_, AppState>, prompt: String) -> AppResult<LiveDashboard> {
    state.ensure_unlocked()?;
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(AppError::invalid("Describe the dashboard"));
    }
    let workspace = state.workspace()?;
    progress(&app, "context", "Finding the relevant tables", StepStatus::Running, None);
    let schema = blocking({
        let workspace = workspace.clone();
        let prompt = prompt.clone();
        move || context::build(&workspace, &prompt, &[])
    })
    .await?;
    let tables = schema.tables.iter().map(|table| table.relation.as_str()).collect::<Vec<_>>().join(", ");
    progress(&app, "context", "Finding the relevant tables", StepStatus::Done, Some(tables));
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let spec = state.llm.resolve(&routing, Task::Sql).ok_or(LlmError::NoModel)?;
    progress(&app, "model", format!("Starting {}", spec.id), StepStatus::Running, None);
    let provider = state.llm.provider(spec.id).await?;
    progress(&app, "model", format!("Starting {}", spec.id), StepStatus::Done, None);
    progress(&app, "design", "Designing the dashboard", StepStatus::Running, None);
    let response = provider
        .chat(
            ChatRequest {
                messages: vec![
                    ChatMessage { role: Role::System, content: prompts::dashboard_system(&schema.prompt) },
                    ChatMessage { role: Role::User, content: prompt.clone() },
                ],
                temperature: prompts::SQL_TEMPERATURE,
                max_tokens: prompts::SQL_MAX_TOKENS,
                format: ResponseFormat::JsonSchema { name: "dashboard".into(), schema: prompts::dashboard_schema() },
            },
            None,
        )
        .await?;
    let mut proposed: Proposed = serde_json::from_str(response.content.trim())
        .map_err(|_| AppError::invalid("The model returned an unreadable dashboard. Try again."))?;
    progress(
        &app,
        "design",
        "Designing the dashboard",
        StepStatus::Done,
        Some(format!("{} · {} tiles", proposed.name.trim(), proposed.tiles.len())),
    );

    // Tiles may use a :name the model forgot to declare; declare it so it shows as a filter.
    for tile in &proposed.tiles {
        for name in sql_params(&tile.sql) {
            if !proposed.filters.iter().any(|filter| filter.name == name) {
                let kind = if name.contains("date") || name.ends_with("_from") || name.ends_with("_to") { "date" } else { "text" };
                proposed.filters.push(FilterDef { label: name.replace('_', " "), name, kind: kind.to_owned() });
            }
        }
    }
    // A text filter nobody has set means "all", so `column = :name` must not drop every row.
    for tile in &mut proposed.tiles {
        for filter in proposed.filters.iter().filter(|filter| filter.kind != "date") {
            tile.sql = optional_equals(&tile.sql, &filter.name);
        }
    }

    let policy = policy(&workspace).await?;
    let defaults = default_params(&proposed.filters);
    let dashboard_id = new_id();
    let mut tiles = Vec::new();
    let mut warnings = Vec::new();
    for (index, proposed) in proposed.tiles.into_iter().enumerate() {
        let title = proposed.title.trim();
        if title.is_empty() || proposed.sql.trim().is_empty() {
            continue;
        }
        let step_id = format!("tile-{index}");
        let step_label = format!("Checking {title}");
        progress(&app, &step_id, &step_label, StepStatus::Running, None);
        let ran = blocking({
            let workspace = workspace.clone();
            let sql = proposed.sql.clone();
            let policy = policy.clone();
            let defaults = defaults.clone();
            move || run_sql(&workspace, &sql, &policy, &defaults).map_err(AppError::invalid)
        })
        .await;
        match ran {
            Ok((sql, result, relations)) => {
                progress(&app, &step_id, &step_label, StepStatus::Done, Some(format!("{} rows", result.rows.len())));
                let hint = chart::hint_of(&proposed.chart);
                let id = new_id();
                let spec = chart::pick(&id, &result, hint);
                let layout = next_layout(&tiles, spec.kind);
                let tile_id = id.clone();
                tiles.push(TileRecord {
                    id,
                    dashboard_id: dashboard_id.clone(),
                    title: title.to_owned(),
                    sql,
                    chart: spec,
                    layout,
                    insight: None,
                    facts_hash: None,
                });
                if !relations.is_empty() {
                    let workspace = workspace.clone();
                    let _ = blocking(move || workspace.db()?.replace_query_relations("tile", &tile_id, &relations)).await;
                }
            }
            Err(error) => {
                progress(&app, &step_id, &step_label, StepStatus::Error, Some(error.to_string()));
                warnings.push(format!("{} was excluded: {error}", title));
            }
        }
    }
    if tiles.is_empty() {
        return Err(AppError::invalid(format!(
            "No tile could be kept. {}",
            if warnings.is_empty() { "The model did not write valid SQL.".to_owned() } else { warnings.join(" ") }
        )));
    }
    let name = if proposed.name.trim().is_empty() { "Dashboard".to_owned() } else { proposed.name.trim().to_owned() };
    let filters = proposed.filters;
    progress(&app, "save", "Saving the dashboard", StepStatus::Running, None);
    let stored = blocking({
        let workspace = workspace.clone();
        let (id, name, prompt, filters, tiles) = (dashboard_id.clone(), name, prompt, filters, tiles);
        move || workspace.db()?.insert_dashboard(&id, &name, None, &prompt, &filters, None, &tiles)
    })
    .await?;
    if !defaults.is_empty() {
        let (id, params_json) = (stored.id.clone(), serde_json::to_string(&defaults)?);
        let workspace = workspace.clone();
        blocking(move || workspace.db()?.set_last_params(&id, &params_json)).await?;
    }
    let live = live_tiles(&state, &workspace, &stored.id, stored.tiles.clone(), &defaults).await?;
    progress(&app, "save", "Saving the dashboard", StepStatus::Done, None);
    Ok(LiveDashboard { dashboard: stored, tiles: live, warnings })
}

#[tauri::command]
pub async fn dashboard_add_tile(state: State<'_, AppState>, dashboard_id: String, title: String, sql: String, chart: Option<String>) -> AppResult<TileRecord> {
    state.ensure_unlocked()?;
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err(AppError::invalid("Name the tile"));
    }
    let workspace = state.workspace()?;
    let policy = policy(&workspace).await?;
    let (sql, result, relations) = blocking({
        let workspace = workspace.clone();
        let sql = sql.clone();
        let policy = policy.clone();
        move || run_sql(&workspace, &sql, &policy, &serde_json::Map::new()).map_err(AppError::invalid)
    })
    .await?;
    let id = new_id();
    let spec = chart::pick(&id, &result, chart::hint_of(chart.as_deref().unwrap_or("auto")));
    let dashboard = blocking({
        let workspace = workspace.clone();
        let dashboard_id = dashboard_id.clone();
        move || workspace.db()?.dashboard(&dashboard_id)
    })
    .await?;
    let layout = next_layout(&dashboard.tiles, spec.kind);
    let tile = TileRecord { id, dashboard_id, title, sql, chart: spec, layout, insight: None, facts_hash: None };
    blocking({
        let workspace = workspace.clone();
        let tile = tile.clone();
        let relations = relations.clone();
        move || {
            let db = workspace.db()?;
            db.insert_tile(&tile)?;
            db.replace_query_relations("tile", &tile.id, &relations)?;
            db.snapshot(&tile.dashboard_id)
        }
    })
    .await?;
    Ok(tile)
}

#[tauri::command]
pub async fn dashboard_add_text_tile(state: State<'_, AppState>, dashboard_id: String, title: String, markdown: String) -> AppResult<TileRecord> {
    let workspace = state.workspace()?;
    let id = new_id();
    let chart = chart::ChartSpec { kind: chart::ChartKind::Text, query_id: id.clone(), x: None, y: Vec::new(), dashed: Vec::new() };
    let title = if title.trim().is_empty() { "Note".to_owned() } else { title.trim().to_owned() };
    let tile = blocking({
        let workspace = workspace.clone();
        move || {
            let db = workspace.db()?;
            let dashboard = db.dashboard(&dashboard_id)?;
            let layout = next_layout(&dashboard.tiles, chart::ChartKind::Text);
            let tile = TileRecord { id, dashboard_id, title, sql: markdown, chart, layout, insight: None, facts_hash: None };
            db.insert_tile(&tile)?;
            db.snapshot(&tile.dashboard_id)?;
            Ok(tile)
        }
    })
    .await?;
    Ok(tile)
}

#[tauri::command]
pub async fn dashboard_update_text_tile(state: State<'_, AppState>, id: String, title: String, markdown: String) -> AppResult<()> {
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        let dashboard_id = db.set_tile_text(&id, title.trim(), &markdown)?;
        db.snapshot(&dashboard_id)
    })
    .await
}

#[tauri::command]
pub async fn dashboard_update_tile(state: State<'_, AppState>, id: String, title: String, sql: String, chart: Option<String>) -> AppResult<()> {
    state.ensure_unlocked()?;
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err(AppError::invalid("Name the tile"));
    }
    let workspace = state.workspace()?;
    let policy = policy(&workspace).await?;
    let (sql, result, relations) = blocking({
        let workspace = workspace.clone();
        let policy = policy.clone();
        move || run_sql(&workspace, &sql, &policy, &serde_json::Map::new()).map_err(AppError::invalid)
    })
    .await?;
    let spec = chart::pick(&id, &result, chart::hint_of(chart.as_deref().unwrap_or("auto")));
    blocking(move || {
        let db = workspace.db()?;
        let dashboard_id = db.set_tile_query(&id, &title, &sql, &spec)?;
        db.replace_query_relations("tile", &id, &relations)?;
        db.snapshot(&dashboard_id)
    })
    .await
}

#[tauri::command]
pub async fn dashboard_delete_tile(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        db.clear_query_relations("tile", &id)?;
        db.delete_tile(&id)
    })
    .await
}

#[tauri::command]
pub async fn dashboard_save_layout(state: State<'_, AppState>, tiles: Vec<LayoutPatch>) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || {
        let layouts = tiles.into_iter().map(|tile| (tile.id, TileLayout { x: tile.x, y: tile.y, w: tile.w, h: tile.h })).collect::<Vec<_>>();
        workspace.db()?.update_layouts(&layouts)
    })
    .await
}

#[tauri::command]
pub async fn dashboard_update(
    state: State<'_, AppState>,
    id: String,
    name: String,
    folder_id: Option<String>,
    filters: Vec<FilterDef>,
    refresh_secs: Option<i64>,
    app: AppHandle,
) -> AppResult<DashboardRecord> {
    state.ensure_unlocked()?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(AppError::invalid("Name the dashboard"));
    }
    let workspace = state.workspace()?;
    let record = blocking({
        let workspace = workspace.clone();
        let folder_id = folder_id.clone();
        let filters = filters.clone();
        move || workspace.db()?.update_dashboard(&id, &name, folder_id.as_deref(), &filters, refresh_secs)
    })
    .await?;
    schedule(&state, &app, &record.id, record.refresh_secs);
    Ok(record)
}

#[tauri::command]
pub async fn dashboard_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    state.refresh.stop(&id);
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_dashboard(&id)).await
}

#[tauri::command]
pub async fn dashboard_duplicate(state: State<'_, AppState>, id: String) -> AppResult<DashboardRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || {
        let source = workspace.db()?.dashboard(&id)?;
        let new_id = uuid::Uuid::new_v4().to_string();
        let tiles = source
            .tiles
            .into_iter()
            .map(|mut tile| {
                tile.id = uuid::Uuid::new_v4().to_string();
                tile.dashboard_id = new_id.clone();
                tile.chart.query_id = tile.id.clone();
                tile
            })
            .collect::<Vec<_>>();
        workspace.db()?.insert_dashboard(&new_id, &format!("{} copy", source.name), source.folder_id.as_deref(), &source.prompt, &source.filters, source.refresh_secs, &tiles)
    })
    .await
}

#[tauri::command]
pub async fn dashboard_versions(state: State<'_, AppState>, id: String) -> AppResult<Vec<DashboardVersion>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.versions(&id)).await
}

#[tauri::command]
pub async fn dashboard_restore(state: State<'_, AppState>, id: String, version_id: String) -> AppResult<DashboardRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.restore_dashboard(&id, &version_id)).await
}

#[tauri::command]
pub async fn dashboard_refresh(state: State<'_, AppState>, id: String, params: Value) -> AppResult<LiveDashboard> {
    dashboard_get(state, id, params).await
}

pub fn resume_dashboards(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let Ok(workspace) = state.workspace() else { return };
    let Ok(items) = workspace.db().and_then(|db| db.scheduled_dashboards()) else { return };
    for (id, secs) in items {
        schedule(&state, app, &id, Some(secs));
    }
}

fn schedule(state: &AppState, app: &AppHandle, id: &str, secs: Option<i64>) {
    state.refresh.stop(id);
    let Some(secs) = secs.filter(|value| *value > 0) else { return };
    let app = app.clone();
    let dashboard_id = id.to_owned();
    let handle = tauri::async_runtime::spawn({
        let id = dashboard_id.clone();
        async move {
        let mut interval = tokio::time::interval(Duration::from_secs(secs as u64));
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(state) = app.try_state::<AppState>() else { break };
            let workspace = match state.workspace() {
                Ok(workspace) => workspace,
                Err(_) => continue,
            };
            let raw = match blocking({
                let workspace = workspace.clone();
                let id = id.clone();
                move || workspace.db()?.last_params(&id)
            })
            .await
            {
                Ok(json) => serde_json::from_str::<serde_json::Map<String, Value>>(&json).unwrap_or_default(),
                Err(_) => continue,
            };
            let dashboard = match blocking({
                let workspace = workspace.clone();
                let id = id.clone();
                move || workspace.db()?.dashboard(&id)
            })
            .await
            {
                Ok(dashboard) => dashboard,
                Err(_) => continue,
            };
            if live_tiles(&state, &workspace, &id, dashboard.tiles, &raw).await.is_ok() {
                let _ = app.emit(DASHBOARD_EVENT, json!({ "id": id }));
            }
        }
        }
    });
    state.refresh.track(dashboard_id, handle);
}
