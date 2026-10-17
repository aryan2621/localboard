//! Generate, convert, serve and export datasites, PDF reports and decks.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::engine::analyst::chart::ChartSpec;
use crate::engine::analyst::{chart, context, prompts, Answer, StepStatus};
use super::progress;
use crate::engine::artifacts::{
    self, html, pdf, pptx, ProposedDatasite, ProposedDeck, ProposedPage, ProposedReport, ARTIFACT_MAX_TOKENS,
};
use crate::engine::query::{self, elapsed_ms, CancelToken, QueryResult};
use crate::error::{blocking, AppError, AppResult};
use crate::llm::provider::{ChatMessage, ChatRequest, LlmError, LlmProvider, ResponseFormat, Role, Task};
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::sql_guard::{self, GuardPolicy};
use crate::state::{AppState, Workspace};
use crate::storage::artifacts::{
    ArtifactSummary, DatasiteChart, DatasitePage, DatasiteRecord, DeckRecord, DeckSlide, OutlineSlide, ReportBlock, ReportRecord,
};
use crate::storage::conversations::{QueryRecord, QueryStatus};
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};

pub const LOCAL_PORT: u16 = 17_890;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedDatasite {
    pub datasite: DatasiteRecord,
    pub url: String,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedReport {
    pub report: ReportRecord,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedDeck {
    pub deck: DeckRecord,
    pub warnings: Vec<String>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn origin(state: &AppState) -> String {
    state.server_origin().unwrap_or_else(|| format!("http://127.0.0.1:{LOCAL_PORT}"))
}

pub fn site_url(origin: &str, id: &str) -> String {
    format!("{origin}/d/{id}")
}

async fn policy(workspace: &std::sync::Arc<Workspace>) -> AppResult<GuardPolicy> {
    let workspace = workspace.clone();
    blocking(move || Ok(GuardPolicy::new(workspace.db()?.list_tables()?.into_iter().map(|table| table.relation)))).await
}

struct Ran {
    sql: String,
    result: QueryResult,
    relations: Vec<String>,
    duration_ms: i64,
}

fn run_sql(workspace: &Workspace, sql: &str, policy: &GuardPolicy) -> Result<Ran, String> {
    let started = Instant::now();
    let validated = sql_guard::validate(sql, policy).map_err(|error| error.to_string())?;
    let result = query::run(
        &workspace.query_connection().map_err(|error| error.to_string())?,
        &validated,
        &std::collections::HashMap::new(),
        &CancelToken::default(),
    )
    .map_err(|error| error.to_string())?;
    Ok(Ran { sql: validated.sql, result, relations: validated.relations, duration_ms: elapsed_ms(started) })
}

fn record_kept(workspace: &Workspace, audit_kind: &str, lineage_kind: &str, ref_id: &str, title: &str, ran: &Ran, detail: Value) {
    if let Ok(db) = workspace.db() {
        db.try_record_audit_lineage(
            &AuditEntry {
                kind: audit_kind,
                status: AuditStatus::Ok,
                duration_ms: Some(ran.duration_ms),
                query: AuditQuery { question: Some(title), sql: Some(&ran.sql), rows: Some(ran.result.rows.len() as i64) },
                detail,
            },
            &ran.relations,
        );
        if !ran.relations.is_empty() {
            let _ = db.replace_query_relations(lineage_kind, ref_id, &ran.relations);
        }
    }
}

fn record_refresh(
    workspace: &Workspace,
    audit_kind: &str,
    lineage_kind: &str,
    ref_id: &str,
    title: &str,
    sql: &str,
    policy: &GuardPolicy,
    detail: Value,
) -> Result<(String, QueryResult), String> {
    let outcome = run_sql(workspace, sql, policy);
    let (status, sql_text, rows, relations, error) = match &outcome {
        Ok(ran) => (AuditStatus::Ok, ran.sql.clone(), Some(ran.result.rows.len() as i64), ran.relations.clone(), None),
        Err(message) => {
            let denied = sql_guard::validate(sql, policy).is_err();
            (if denied { AuditStatus::Denied } else { AuditStatus::Error }, sql.to_owned(), None, Vec::new(), Some(message.clone()))
        }
    };
    if let Ok(db) = workspace.db() {
        let mut detail = detail;
        if let Some(error) = &error {
            detail["error"] = json!(error);
        }
        db.try_record_audit_lineage(
            &AuditEntry {
                kind: audit_kind,
                status,
                duration_ms: outcome.as_ref().ok().map(|ran| ran.duration_ms),
                query: AuditQuery { question: Some(title), sql: Some(&sql_text), rows },
                detail,
            },
            &relations,
        );
        if !relations.is_empty() {
            let _ = db.replace_query_relations(lineage_kind, ref_id, &relations);
        }
    }
    match outcome {
        Ok(ran) => Ok((ran.sql, ran.result)),
        Err(error) => Err(error),
    }
}

async fn llm_json<T: serde::de::DeserializeOwned>(state: &AppState, system: String, prompt: String, name: &str, schema: Value) -> AppResult<T> {
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let spec = state.llm.resolve(&routing, Task::Sql).ok_or(LlmError::NoModel)?;
    let provider = state.llm.provider(spec.id).await?;
    let response = provider
        .chat(
            ChatRequest {
                messages: vec![ChatMessage { role: Role::System, content: system }, ChatMessage { role: Role::User, content: prompt }],
                temperature: prompts::SQL_TEMPERATURE,
                max_tokens: ARTIFACT_MAX_TOKENS,
                format: ResponseFormat::JsonSchema { name: name.into(), schema },
            },
            None,
        )
        .await?;
    serde_json::from_str(response.content.trim()).map_err(|_| AppError::invalid("The model returned an unreadable result. Try again."))
}

async fn narrate(state: &AppState, title: &str, facts: &Value) -> String {
    let routing: TaskRouting = match state.app_db() {
        Ok(db) => db.setting(SETTINGS_KEY).ok().flatten().unwrap_or_default(),
        Err(_) => return String::new(),
    };
    let spec = match state.llm.resolve(&routing, Task::Narrative) {
        Some(spec) => spec,
        None => return String::new(),
    };
    let Ok(provider) = state.llm.provider(spec.id).await else { return String::new() };
    let Ok(response) = provider
        .chat(
            ChatRequest {
                messages: vec![
                    ChatMessage { role: Role::System, content: prompts::NARRATIVE_SYSTEM.to_owned() },
                    ChatMessage { role: Role::User, content: prompts::narrative_user(title, facts) },
                ],
                temperature: prompts::NARRATIVE_TEMPERATURE,
                max_tokens: prompts::NARRATIVE_MAX_TOKENS,
                format: ResponseFormat::Text,
            },
            None,
        )
        .await
    else {
        return String::new();
    };
    response.content.trim().to_owned()
}

/// Builds the schema text for a prompt, reporting the tables it found.
async fn staged_schema(app: Option<&tauri::AppHandle>, workspace: &std::sync::Arc<Workspace>, prompt: &str) -> AppResult<String> {
    progress(app, "context", "Finding the relevant tables", StepStatus::Running, None);
    let workspace = workspace.clone();
    let prompt = prompt.to_owned();
    let schema = blocking(move || context::build(&workspace, &prompt, &[])).await?;
    let tables = schema.tables.iter().map(|table| table.relation.as_str()).collect::<Vec<_>>().join(", ");
    progress(app, "context", "Finding the relevant tables", StepStatus::Done, Some(tables));
    Ok(schema.prompt)
}

fn facts_value(result: &QueryResult) -> Value {
    artifacts::facts_of(result)
}

fn first_line(text: &str, fallback: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or(fallback).trim();
    if line.is_empty() {
        fallback.to_owned()
    } else if line.chars().count() > 60 {
        format!("{}…", line.chars().take(59).collect::<String>())
    } else {
        line.to_owned()
    }
}

pub async fn load_answer(workspace: &std::sync::Arc<Workspace>, message_id: &str) -> AppResult<(Answer, String, Vec<QueryRecord>)> {
    let workspace = workspace.clone();
    let message_id = message_id.to_owned();
    blocking(move || {
        let db = workspace.db()?;
        let message = db.message(&message_id)?;
        let answer = message.answer.as_ref().and_then(|value| serde_json::from_value::<Answer>(value.clone()).ok()).unwrap_or_default();
        let queries = db
            .conversation_queries(&message.conversation_id)?
            .into_iter()
            .filter(|query| query.message_id == message_id && query.status == QueryStatus::Ok)
            .collect();
        Ok((answer, message.content, queries))
    })
    .await
}

struct KeptChart {
    title: String,
    sql: String,
    spec: ChartSpec,
    result: QueryResult,
    facts: Value,
}

async fn keep_sql(
    workspace: &std::sync::Arc<Workspace>,
    policy: &GuardPolicy,
    title: &str,
    sql: &str,
    hint: &str,
    audit_kind: &str,
    lineage_kind: &str,
    ref_id: &str,
    detail: Value,
) -> Result<KeptChart, String> {
    let ran = blocking({
        let workspace = workspace.clone();
        let sql = sql.to_owned();
        let policy = policy.clone();
        move || run_sql(&workspace, &sql, &policy).map_err(AppError::invalid)
    })
    .await
    .map_err(|error| error.to_string())?;
    record_kept(workspace, audit_kind, lineage_kind, ref_id, title, &ran, detail);
    let spec = chart::pick(ref_id, &ran.result, chart::hint_of(hint));
    Ok(KeptChart { title: title.to_owned(), sql: ran.sql, spec, facts: facts_value(&ran.result), result: artifacts::snap(&ran.result) })
}

fn attach_lineage(workspace: &std::sync::Arc<Workspace>, kind: &'static str, id: String, relations: Vec<String>) {
    if relations.is_empty() {
        return;
    }
    let workspace = workspace.clone();
    tauri::async_runtime::spawn(async move {
        let _ = blocking(move || workspace.db()?.replace_query_relations(kind, &id, &relations)).await;
    });
}

async fn convert_sql(policy: &GuardPolicy, sql: &str, relations: &[String]) -> Result<(String, Vec<String>), String> {
    let validated = sql_guard::validate(sql, policy).map_err(|error| error.to_string())?;
    let rels = if validated.relations.is_empty() { relations.to_vec() } else { validated.relations };
    Ok((validated.sql, rels))
}

pub async fn refresh_datasite_chart(app: &AppHandle, datasite_id: &str, chart_id: &str) -> AppResult<QueryResult> {
    let state = app.state::<AppState>();
    let workspace = state.workspace()?;
    let site = blocking({
        let workspace = workspace.clone();
        let id = datasite_id.to_owned();
        move || workspace.db()?.datasite(&id)
    })
    .await?;
    let chart = site
        .pages
        .iter()
        .flat_map(|page| page.charts.iter())
        .find(|chart| chart.id == chart_id)
        .cloned()
        .ok_or_else(|| AppError::not_found("Chart not found"))?;
    let policy = policy(&workspace).await?;
    let page_id = chart.page_id.clone();
    let (_, result) = blocking({
        let workspace = workspace.clone();
        let policy = policy.clone();
        let title = chart.title.clone();
        let sql = chart.sql.clone();
        let chart_id = chart.id.clone();
        let datasite_id = datasite_id.to_owned();
        move || {
            record_refresh(
                &workspace,
                "datasite.chart",
                "datasite",
                &chart_id,
                &title,
                &sql,
                &policy,
                json!({ "datasite": datasite_id, "page": page_id, "chart": chart_id }),
            )
            .map_err(AppError::invalid)
        }
    })
    .await?;
    let facts = facts_value(&result);
    blocking({
        let workspace = workspace.clone();
        let id = chart.id.clone();
        let result = result.clone();
        move || workspace.db()?.set_datasite_chart_snapshot(&id, &result, &facts)
    })
    .await?;
    Ok(result)
}

#[tauri::command]
pub async fn datasite_list(state: State<'_, AppState>) -> AppResult<Vec<ArtifactSummary>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_datasites()).await
}

#[tauri::command]
pub async fn datasite_get(state: State<'_, AppState>, id: String) -> AppResult<GeneratedDatasite> {
    let workspace = state.workspace()?;
    let datasite = blocking({
        let workspace = workspace.clone();
        move || workspace.db()?.datasite(&id)
    })
    .await?;
    Ok(GeneratedDatasite { url: site_url(&origin(&state), &datasite.id), datasite, warnings: Vec::new() })
}

#[tauri::command]
pub async fn datasite_generate(app: tauri::AppHandle, state: State<'_, AppState>, prompt: String) -> AppResult<GeneratedDatasite> {
    state.ensure_unlocked()?;
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(AppError::invalid("Describe the datasite"));
    }
    let app = Some(&app);
    let workspace = state.workspace()?;
    let schema = staged_schema(app, &workspace, &prompt).await?;
    progress(app, "design", "Designing the datasite", StepStatus::Running, None);
    let proposed: ProposedDatasite = llm_json(&state, artifacts::datasite_system(&schema), prompt.clone(), "datasite", artifacts::datasite_schema()).await?;
    progress(app, "design", "Designing the datasite", StepStatus::Done, Some(format!("{} · {} sections", proposed.name.trim(), proposed.sections.len())));
    progress(app, "build", "Running the queries and saving", StepStatus::Running, None);
    let built = assemble_datasite(&state, &workspace, artifacts::display_name(&proposed.name, "Datasite"), &prompt, None, proposed.overview, proposed.sections).await?;
    progress(app, "build", "Running the queries and saving", StepStatus::Done, None);
    Ok(built)
}

#[tauri::command]
pub async fn datasite_from_answer(state: State<'_, AppState>, message_id: String) -> AppResult<GeneratedDatasite> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let (answer, content, queries) = load_answer(&workspace, &message_id).await?;
    let policy = policy(&workspace).await?;
    let items = artifacts::answer_charts(&answer, &queries);
    if items.is_empty() {
        return Err(AppError::invalid("This answer has no kept query to save."));
    }
    let site_id = new_id();
    let page_id = new_id();
    let mut charts = Vec::new();
    let mut warnings = Vec::new();
    for (index, (query_id, title, spec, result, relations)) in items.into_iter().enumerate() {
        let sql = queries.iter().find(|query| query.id == query_id).map(|query| query.sql.as_str()).unwrap_or("");
        match convert_sql(&policy, sql, &relations).await {
            Ok((sql, rels)) => {
                let chart_id = new_id();
                attach_lineage(&workspace, "datasite", chart_id.clone(), rels);
                charts.push(DatasiteChart {
                    id: chart_id,
                    page_id: page_id.clone(),
                    title,
                    sql,
                    chart: ChartSpec { query_id: spec.query_id, ..spec },
                    result: Some(artifacts::snap(&result)),
                    facts: Some(facts_value(&result)),
                    sort: index as i64,
                });
            }
            Err(error) => warnings.push(format!("{title} was excluded: {error}")),
        }
    }
    if charts.is_empty() {
        return Err(AppError::invalid(if warnings.is_empty() {
            "This answer has no kept query to save.".into()
        } else {
            warnings.join(" ")
        }));
    }
    let narrative = artifacts::answer_text(&answer, &content);
    persist_datasite(
        &state,
        &workspace,
        DatasiteRecord {
            id: site_id.clone(),
            name: first_line(&narrative, "Datasite"),
            prompt: narrative.clone(),
            source_message_id: Some(message_id),
            pages: vec![DatasitePage {
                id: page_id,
                datasite_id: site_id,
                slug: "overview".into(),
                title: "Overview".into(),
                sort: 0,
                narrative,
                charts,
            }],
        },
        warnings,
    )
    .await
}

async fn assemble_datasite(
    state: &AppState,
    workspace: &std::sync::Arc<Workspace>,
    name: String,
    prompt: &str,
    source: Option<String>,
    overview: ProposedPage,
    sections: Vec<ProposedPage>,
) -> AppResult<GeneratedDatasite> {
    let policy = policy(workspace).await?;
    let site_id = new_id();
    let mut pages = Vec::new();
    let mut warnings = Vec::new();
    let mut slugs = HashSet::new();
    slugs.insert("overview".to_owned());
    let mut incoming = vec![("overview".to_owned(), overview)];
    for section in sections {
        incoming.push((artifacts::slug(&section.title, &mut slugs), section));
    }
    for (sort, (slug, proposed)) in incoming.into_iter().enumerate() {
        let page_id = new_id();
        let mut charts = Vec::new();
        let mut page_facts = Vec::new();
        for (index, tile) in proposed.charts.into_iter().enumerate() {
            let title = tile.title.trim().to_owned();
            if title.is_empty() || tile.sql.trim().is_empty() {
                continue;
            }
            let chart_id = new_id();
            match keep_sql(
                workspace,
                &policy,
                &title,
                &tile.sql,
                &tile.chart,
                "datasite.chart",
                "datasite",
                &chart_id,
                json!({ "datasite": site_id, "page": page_id, "chart": chart_id }),
            )
            .await
            {
                Ok(kept) => {
                    page_facts.push(kept.facts.clone());
                    charts.push(DatasiteChart {
                        id: chart_id,
                        page_id: page_id.clone(),
                        title: kept.title,
                        sql: kept.sql,
                        chart: kept.spec,
                        result: Some(kept.result),
                        facts: Some(kept.facts),
                        sort: index as i64,
                    });
                }
                Err(error) => warnings.push(format!("{title} was excluded: {error}")),
            }
        }
        let title = if proposed.title.trim().is_empty() { if slug == "overview" { "Overview".into() } else { format!("Section {}", sort + 1) } } else { proposed.title.trim().to_owned() };
        let narrative = if page_facts.is_empty() {
            proposed.narrative.trim().to_owned()
        } else {
            let written = narrate(state, &title, &json!({ "charts": page_facts })).await;
            if written.is_empty() { proposed.narrative.trim().to_owned() } else { written }
        };
        pages.push(DatasitePage { id: page_id, datasite_id: site_id.clone(), slug, title, sort: sort as i64, narrative, charts });
    }
    if pages.iter().all(|page| page.charts.is_empty()) {
        return Err(AppError::invalid(if warnings.is_empty() {
            "No page could be kept. The model did not write valid SQL.".into()
        } else {
            warnings.join(" ")
        }));
    }
    persist_datasite(
        state,
        workspace,
        DatasiteRecord { id: site_id, name, prompt: prompt.to_owned(), source_message_id: source, pages },
        warnings,
    )
    .await
}

async fn persist_datasite(
    state: &AppState,
    workspace: &std::sync::Arc<Workspace>,
    record: DatasiteRecord,
    warnings: Vec<String>,
) -> AppResult<GeneratedDatasite> {
    let stored = blocking({
        let workspace = workspace.clone();
        move || workspace.db()?.insert_datasite(&record)
    })
    .await?;
    Ok(GeneratedDatasite { url: site_url(&origin(state), &stored.id), datasite: stored, warnings })
}

#[tauri::command]
pub async fn datasite_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_datasite(&id)).await
}

#[tauri::command]
pub async fn datasite_export(state: State<'_, AppState>, id: String, mode: String, path: PathBuf) -> AppResult<()> {
    let workspace = state.workspace()?;
    let site = blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || workspace.db()?.datasite(&id)
    })
    .await?;
    let theme = blocking({
        let workspace = workspace.clone();
        move || workspace.db()?.theme()
    })
    .await
    .unwrap_or(crate::storage::theme::WorkspaceTheme {
        brand: artifacts::BRAND_HEX.into(),
        name: artifacts::BRAND_NAME.into(),
    });
    let html = match mode.as_str() {
        "live" => html::datasite_bundle(&site, html::SiteMode::Linked { origin: &origin(&state) }, &theme.brand, &theme.name),
        "static" => html::datasite_bundle(&site, html::SiteMode::Static, &theme.brand, &theme.name),
        _ => return Err(AppError::invalid("Export is static or live.")),
    };
    std::fs::write(path, html)?;
    Ok(())
}

#[tauri::command]
pub async fn report_list(state: State<'_, AppState>) -> AppResult<Vec<ArtifactSummary>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_reports()).await
}

#[tauri::command]
pub async fn report_get(state: State<'_, AppState>, id: String) -> AppResult<GeneratedReport> {
    let workspace = state.workspace()?;
    let report = blocking(move || workspace.db()?.report(&id)).await?;
    Ok(GeneratedReport { report, warnings: Vec::new() })
}

pub async fn generate_report(state: &AppState, prompt: String, app: Option<&tauri::AppHandle>) -> AppResult<GeneratedReport> {
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(AppError::invalid("Describe the report"));
    }
    let workspace = state.workspace()?;
    let schema = staged_schema(app, &workspace, &prompt).await?;
    progress(app, "design", "Designing the report", StepStatus::Running, None);
    let proposed: ProposedReport = llm_json(state, artifacts::report_system(&schema), prompt.clone(), "report", artifacts::report_schema()).await?;
    progress(app, "design", "Designing the report", StepStatus::Done, Some(format!("{} · {} blocks", proposed.name.trim(), proposed.blocks.len())));
    progress(app, "build", "Running the queries and saving", StepStatus::Running, None);
    let built = assemble_report(state, &workspace, artifacts::display_name(&proposed.name, "Report"), &prompt, None, proposed.blocks).await?;
    progress(app, "build", "Running the queries and saving", StepStatus::Done, None);
    Ok(built)
}

#[tauri::command]
pub async fn report_generate(app: tauri::AppHandle, state: State<'_, AppState>, prompt: String) -> AppResult<GeneratedReport> {
    state.ensure_unlocked()?;
    generate_report(&state, prompt, Some(&app)).await
}

#[tauri::command]
pub async fn report_from_answer(state: State<'_, AppState>, message_id: String) -> AppResult<GeneratedReport> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let (answer, content, queries) = load_answer(&workspace, &message_id).await?;
    let policy = policy(&workspace).await?;
    let items = artifacts::answer_charts(&answer, &queries);
    if items.is_empty() {
        return Err(AppError::invalid("This answer has no kept query to save."));
    }
    let report_id = new_id();
    let narrative = artifacts::answer_text(&answer, &content);
    let mut blocks = vec![ReportBlock {
        id: new_id(),
        report_id: report_id.clone(),
        kind: "narrative".into(),
        title: "Findings".into(),
        body: narrative.clone(),
        sql: None,
        chart: None,
        result: None,
        facts: None,
        sort: 0,
    }];
    let mut warnings = Vec::new();
    for (index, (query_id, title, spec, result, relations)) in items.into_iter().enumerate() {
        let sql = queries.iter().find(|query| query.id == query_id).map(|query| query.sql.as_str()).unwrap_or("");
        match convert_sql(&policy, sql, &relations).await {
            Ok((sql, rels)) => {
                let block_id = new_id();
                attach_lineage(&workspace, "report", block_id.clone(), rels);
                blocks.push(ReportBlock {
                    id: block_id,
                    report_id: report_id.clone(),
                    kind: "chart".into(),
                    title,
                    body: String::new(),
                    sql: Some(sql),
                    chart: Some(spec),
                    result: Some(artifacts::snap(&result)),
                    facts: Some(facts_value(&result)),
                    sort: (index + 1) as i64,
                });
            }
            Err(error) => warnings.push(format!("{title} was excluded: {error}")),
        }
    }
    persist_report(
        &workspace,
        ReportRecord {
            id: report_id,
            name: first_line(&narrative, "Report"),
            prompt: narrative,
            theme: artifacts::default_theme(),
            source_message_id: Some(message_id),
            blocks,
        },
        warnings,
    )
    .await
}

async fn assemble_report(
    state: &AppState,
    workspace: &std::sync::Arc<Workspace>,
    name: String,
    prompt: &str,
    source: Option<String>,
    proposed: Vec<crate::engine::artifacts::ProposedBlock>,
) -> AppResult<GeneratedReport> {
    let policy = policy(workspace).await?;
    let report_id = new_id();
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    for (index, item) in proposed.into_iter().enumerate() {
        let title = item.title.trim().to_owned();
        if item.kind == "narrative" {
            let body = item.body.trim().to_owned();
            if title.is_empty() && body.is_empty() {
                continue;
            }
            blocks.push(ReportBlock {
                id: new_id(),
                report_id: report_id.clone(),
                kind: "narrative".into(),
                title,
                body,
                sql: None,
                chart: None,
                result: None,
                facts: None,
                sort: index as i64,
            });
            continue;
        }
        if title.is_empty() || item.sql.trim().is_empty() {
            continue;
        }
        let block_id = new_id();
        match keep_sql(
            workspace,
            &policy,
            &title,
            &item.sql,
            &item.chart,
            "report.block",
            "report",
            &block_id,
            json!({ "report": report_id, "block": block_id }),
        )
        .await
        {
            Ok(kept) => {
                let body = narrate(state, &title, &kept.facts).await;
                blocks.push(ReportBlock {
                    id: block_id,
                    report_id: report_id.clone(),
                    kind: if item.kind == "table" { "table".into() } else { "chart".into() },
                    title: kept.title,
                    body,
                    sql: Some(kept.sql),
                    chart: Some(kept.spec),
                    result: Some(kept.result),
                    facts: Some(kept.facts),
                    sort: index as i64,
                });
            }
            Err(error) => warnings.push(format!("{title} was excluded: {error}")),
        }
    }
    if blocks.iter().all(|block| block.sql.is_none()) {
        return Err(AppError::invalid(if warnings.is_empty() {
            "No report block could be kept. The model did not write valid SQL.".into()
        } else {
            warnings.join(" ")
        }));
    }
    persist_report(
        workspace,
        ReportRecord { id: report_id, name, prompt: prompt.to_owned(), theme: artifacts::default_theme(), source_message_id: source, blocks },
        warnings,
    )
    .await
}

async fn persist_report(workspace: &std::sync::Arc<Workspace>, mut record: ReportRecord, warnings: Vec<String>) -> AppResult<GeneratedReport> {
    if let Ok(theme) = blocking({
        let workspace = workspace.clone();
        move || workspace.db()?.theme()
    })
    .await
    {
        record.theme = artifacts::theme_of(&theme);
    }
    let report = blocking({
        let workspace = workspace.clone();
        move || workspace.db()?.insert_report(&record)
    })
    .await?;
    Ok(GeneratedReport { report, warnings })
}

#[tauri::command]
pub async fn report_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_report(&id)).await
}

#[tauri::command]
pub async fn report_export_pdf(state: State<'_, AppState>, id: String, path: PathBuf) -> AppResult<()> {
    let workspace = state.workspace()?;
    let policy = policy(&workspace).await?;
    let mut report = blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || workspace.db()?.report(&id)
    })
    .await?;
    for block in &mut report.blocks {
        let Some(sql) = block.sql.clone() else { continue };
        let refreshed = blocking({
            let workspace = workspace.clone();
            let policy = policy.clone();
            let title = block.title.clone();
            let block_id = block.id.clone();
            let report_id = report.id.clone();
            move || {
                record_refresh(
                    &workspace,
                    "report.block",
                    "report",
                    &block_id,
                    &title,
                    &sql,
                    &policy,
                    json!({ "report": report_id, "block": block_id }),
                )
                .map_err(AppError::invalid)
            }
        })
        .await;
        if let Ok((_, result)) = refreshed {
            let facts = facts_value(&result);
            let id = block.id.clone();
            block.result = Some(result.clone());
            block.facts = Some(facts.clone());
            let workspace = workspace.clone();
            let _ = blocking(move || workspace.db()?.set_report_block_snapshot(&id, &result, &facts)).await;
        }
    }
    let bytes = pdf::write(&report)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[tauri::command]
pub async fn deck_list(state: State<'_, AppState>) -> AppResult<Vec<ArtifactSummary>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_decks()).await
}

#[tauri::command]
pub async fn deck_get(state: State<'_, AppState>, id: String) -> AppResult<GeneratedDeck> {
    let workspace = state.workspace()?;
    let deck = blocking(move || workspace.db()?.deck(&id)).await?;
    Ok(GeneratedDeck { deck, warnings: Vec::new() })
}

pub async fn generate_deck(state: &AppState, prompt: String, app: Option<&tauri::AppHandle>) -> AppResult<GeneratedDeck> {
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(AppError::invalid("Describe the presentation"));
    }
    let workspace = state.workspace()?;
    let schema = staged_schema(app, &workspace, &prompt).await?;
    progress(app, "design", "Designing the presentation", StepStatus::Running, None);
    let proposed: ProposedDeck = llm_json(state, artifacts::deck_system(&schema), prompt.clone(), "deck", artifacts::deck_schema()).await?;
    progress(app, "design", "Designing the presentation", StepStatus::Done, Some(format!("{} · {} slides", proposed.name.trim(), proposed.slides.len())));
    progress(app, "build", "Running the queries and saving", StepStatus::Running, None);
    let built = assemble_deck(state, &workspace, artifacts::display_name(&proposed.name, "Presentation"), &prompt, None, proposed.slides).await?;
    progress(app, "build", "Running the queries and saving", StepStatus::Done, None);
    Ok(built)
}

#[tauri::command]
pub async fn deck_generate(app: tauri::AppHandle, state: State<'_, AppState>, prompt: String) -> AppResult<GeneratedDeck> {
    state.ensure_unlocked()?;
    generate_deck(&state, prompt, Some(&app)).await
}

#[tauri::command]
pub async fn deck_from_answer(state: State<'_, AppState>, message_id: String) -> AppResult<GeneratedDeck> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let (answer, content, queries) = load_answer(&workspace, &message_id).await?;
    let policy = policy(&workspace).await?;
    let items = artifacts::answer_charts(&answer, &queries);
    let deck_id = new_id();
    let narrative = artifacts::answer_text(&answer, &content);
    let mut slides = vec![DeckSlide {
        id: new_id(),
        deck_id: deck_id.clone(),
        title: first_line(&narrative, "Findings"),
        body: narrative.clone(),
        sql: None,
        chart: None,
        result: None,
        sort: 0,
    }];
    let mut warnings = Vec::new();
    for (index, (query_id, title, spec, result, relations)) in items.into_iter().enumerate() {
        let sql = queries.iter().find(|query| query.id == query_id).map(|query| query.sql.as_str()).unwrap_or("");
        match convert_sql(&policy, sql, &relations).await {
            Ok((sql, rels)) => {
                let slide_id = new_id();
                attach_lineage(&workspace, "deck", slide_id.clone(), rels);
                slides.push(DeckSlide {
                    id: slide_id,
                    deck_id: deck_id.clone(),
                    title,
                    body: String::new(),
                    sql: Some(sql),
                    chart: Some(spec),
                    result: Some(artifacts::snap(&result)),
                    sort: (index + 1) as i64,
                });
            }
            Err(error) => warnings.push(format!("{title} was excluded: {error}")),
        }
    }
    if slides.len() == 1 && slides[0].sql.is_none() && queries.is_empty() {
        return Err(AppError::invalid("This answer has no kept query to save."));
    }
    persist_deck(&workspace, deck_from_slides(deck_id, first_line(&narrative, "Presentation"), narrative, Some(message_id), slides), warnings).await
}

async fn assemble_deck(
    state: &AppState,
    workspace: &std::sync::Arc<Workspace>,
    name: String,
    prompt: &str,
    source: Option<String>,
    proposed: Vec<crate::engine::artifacts::ProposedSlide>,
) -> AppResult<GeneratedDeck> {
    let policy = policy(workspace).await?;
    let deck_id = new_id();
    let mut slides = Vec::new();
    let mut warnings = Vec::new();
    for (index, item) in proposed.into_iter().enumerate() {
        let title = item.title.trim().to_owned();
        if title.is_empty() {
            continue;
        }
        if item.sql.trim().is_empty() {
            slides.push(DeckSlide {
                id: new_id(),
                deck_id: deck_id.clone(),
                title,
                body: item.body.trim().to_owned(),
                sql: None,
                chart: None,
                result: None,
                sort: index as i64,
            });
            continue;
        }
        let slide_id = new_id();
        match keep_sql(
            workspace,
            &policy,
            &title,
            &item.sql,
            &item.chart,
            "deck.slide",
            "deck",
            &slide_id,
            json!({ "deck": deck_id, "slide": slide_id }),
        )
        .await
        {
            Ok(kept) => {
                let body = narrate(state, &title, &kept.facts).await;
                let body = if body.is_empty() { item.body.trim().to_owned() } else { body };
                slides.push(DeckSlide {
                    id: slide_id,
                    deck_id: deck_id.clone(),
                    title: kept.title,
                    body,
                    sql: Some(kept.sql),
                    chart: Some(kept.spec),
                    result: Some(kept.result),
                    sort: index as i64,
                });
            }
            Err(error) => warnings.push(format!("{title} was excluded: {error}")),
        }
    }
    if slides.is_empty() {
        return Err(AppError::invalid(if warnings.is_empty() {
            "No slide could be kept.".into()
        } else {
            warnings.join(" ")
        }));
    }
    persist_deck(&workspace, deck_from_slides(deck_id, name, prompt.to_owned(), source, slides), warnings).await
}

fn deck_from_slides(id: String, name: String, prompt: String, source: Option<String>, slides: Vec<DeckSlide>) -> DeckRecord {
    let outline = slides.iter().map(|slide| OutlineSlide { id: slide.id.clone(), title: slide.title.clone(), body: slide.body.clone() }).collect();
    DeckRecord { id, name, prompt, outline, source_message_id: source, slides }
}

async fn persist_deck(workspace: &std::sync::Arc<Workspace>, record: DeckRecord, warnings: Vec<String>) -> AppResult<GeneratedDeck> {
    let deck = blocking({
        let workspace = workspace.clone();
        move || workspace.db()?.insert_deck(&record)
    })
    .await?;
    Ok(GeneratedDeck { deck, warnings })
}

#[tauri::command]
pub async fn deck_update_outline(state: State<'_, AppState>, id: String, outline: Vec<OutlineSlide>) -> AppResult<DeckRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.update_deck_outline(&id, &outline)).await
}

#[tauri::command]
pub async fn deck_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_deck(&id)).await
}

#[tauri::command]
pub async fn deck_export_pptx(state: State<'_, AppState>, id: String, path: PathBuf) -> AppResult<()> {
    let workspace = state.workspace()?;
    let policy = policy(&workspace).await?;
    let mut deck = blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || workspace.db()?.deck(&id)
    })
    .await?;
    let outline = deck.outline.clone();
    for slide in &mut deck.slides {
        if let Some(item) = outline.iter().find(|item| item.id == slide.id) {
            slide.title = item.title.clone();
            slide.body = item.body.clone();
        }
        let Some(sql) = slide.sql.clone() else { continue };
        let refreshed = blocking({
            let workspace = workspace.clone();
            let policy = policy.clone();
            let title = slide.title.clone();
            let slide_id = slide.id.clone();
            let deck_id = deck.id.clone();
            move || {
                record_refresh(
                    &workspace,
                    "deck.slide",
                    "deck",
                    &slide_id,
                    &title,
                    &sql,
                    &policy,
                    json!({ "deck": deck_id, "slide": slide_id }),
                )
                .map_err(AppError::invalid)
            }
        })
        .await;
        if let Ok((_, result)) = refreshed {
            let id = slide.id.clone();
            slide.result = Some(result.clone());
            let workspace = workspace.clone();
            let _ = blocking(move || workspace.db()?.set_deck_slide_snapshot(&id, &result)).await;
        }
    }
    let bytes = pptx::write(&deck)?;
    std::fs::write(path, bytes)?;
    Ok(())
}
