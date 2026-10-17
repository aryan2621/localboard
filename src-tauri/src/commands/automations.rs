//! Workflow automations: triggers, steps, tray, webhook and optional SMTP.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::artifacts::{generate_deck, generate_report};
use crate::connectors::watcher::{self, WatchScope, WatchTarget};
use crate::engine::analyst::{AnalystEvent, EventSink, Models, Run, StepStatus};
use crate::engine::analyst::facts::{self, Facts};
use crate::engine::automations::{self, Threshold};
use crate::engine::query::{self, bind_params, elapsed_ms, CancelToken};
use crate::error::{blocking, AppError, AppResult};
use crate::llm::provider::{ChatMessage, ChatRequest, LlmProvider, ResponseFormat, Role, Task};
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::retrieval;
use crate::sql_guard::{self, GuardPolicy};
use crate::state::AppState;
use crate::storage::automations::{
    hash_token, new_webhook_token, AutomationRecord, AutomationRun, AutomationStep, SmtpSettings,
};
use crate::storage::conversations::{title_from, MessageRole, MessageStatus};
use crate::storage::secrets;
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};

pub const AUTOMATION_EVENT: &str = "automation://ran";
const SMTP_KEY: &str = "smtp";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepDraft {
    pub id: Option<String>,
    pub kind: String,
    pub config: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDraft {
    pub id: Option<String>,
    pub name: String,
    pub trigger_kind: String,
    pub trigger: Value,
    pub steps: Vec<StepDraft>,
    pub enabled: bool,
    pub on_change_only: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpDraft {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub from: String,
    pub password: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookCreated {
    pub url: String,
    pub token: String,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(error) = crate::notifier::show(app, title, body) {
        tracing::warn!("could not show a notification: {error}");
    }
}

fn show_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn smtp_secret(workspace_id: &str) -> String {
    format!("smtp:{workspace_id}")
}

fn webhook_url(origin: &str, id: &str) -> String {
    format!("{origin}/hooks/{id}")
}

fn with_webhook(mut record: AutomationRecord, origin: Option<&str>) -> AutomationRecord {
    if record.trigger_kind == "webhook" {
        if let Some(origin) = origin {
            record.webhook_url = Some(webhook_url(origin, &record.id));
        }
    }
    record
}

fn validate_steps(steps: &[StepDraft]) -> AppResult<Vec<AutomationStep>> {
    if steps.is_empty() {
        return Err(AppError::invalid("Add at least one step"));
    }
    if steps.len() > 20 {
        return Err(AppError::invalid("An automation can have at most 20 steps"));
    }
    let mut out = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        if !automations::valid_step(&step.kind) {
            return Err(AppError::invalid(format!("Unknown step kind: {}", step.kind)));
        }
        out.push(AutomationStep {
            id: step.id.clone().unwrap_or_else(new_id),
            sort: index as i64,
            kind: step.kind.clone(),
            config: step.config.clone(),
        });
    }
    Ok(out)
}

fn validate_trigger(kind: &str, trigger: &Value) -> AppResult<Value> {
    if !automations::valid_trigger(kind) {
        return Err(AppError::invalid("Trigger must be cron, interval, file change, manual or webhook"));
    }
    match kind {
        "interval" => {
            let secs = automations::interval_secs(trigger).ok_or_else(|| AppError::invalid("Interval must be at least 60 seconds"))?;
            Ok(json!({ "intervalSecs": secs }))
        }
        "cron" => {
            let expr = automations::cron_expr(trigger).ok_or_else(|| AppError::invalid("Enter a cron expression"))?;
            let normalized = automations::normalize_cron(expr).map_err(AppError::invalid)?;
            Ok(json!({ "cron": normalized }))
        }
        "file_change" => {
            let path = automations::watch_path(trigger).ok_or_else(|| AppError::invalid("Choose a file or folder to watch"))?;
            Ok(json!({ "path": path }))
        }
        "manual" | "webhook" => Ok(trigger.clone()),
        _ => Ok(trigger.clone()),
    }
}

pub fn install_tray(app: &AppHandle) -> AppResult<()> {
    use tauri::image::Image;
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    let internal = |error: tauri::Error| AppError::Internal(error.to_string());
    let show = MenuItem::with_id(app, "show", "Open LocalBoard", true, None::<&str>).map_err(internal)?;
    let background = MenuItem::with_id(app, "background", "Quit (keep running in background)", true, None::<&str>).map_err(internal)?;
    let force = MenuItem::with_id(app, "force_quit", "Force Quit (stop everything)", true, Some("CmdOrCtrl+Q")).map_err(internal)?;
    let separator = PredefinedMenuItem::separator(app).map_err(internal)?;
    let menu = Menu::with_items(app, &[&show, &separator, &background, &force]).map_err(internal)?;
    TrayIconBuilder::with_id("localboard")
        .icon(Image::from_bytes(include_bytes!("../../icons/tray.png")).map_err(internal)?)
        .icon_as_template(true)
        .tooltip("LocalBoard")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_window(app),
            "background" => crate::commands::system::app_run_in_background(app.clone()),
            "force_quit" => app.exit(0),
            _ => {}
        })
        .build(app)
        .map_err(internal)?;
    Ok(())
}

pub fn resume(app: &AppHandle) {
    crate::commands::dashboards::resume_dashboards(app);
    let Some(state) = app.try_state::<AppState>() else { return };
    state.jobs.stop_all();
    let _ = state.set_file_jobs(None);
    let Ok(workspace) = state.workspace() else { return };
    let Ok(items) = workspace.db().and_then(|db| db.enabled_automations()) else { return };
    let mut watches = Vec::new();
    for item in items {
        schedule(&state, app, &item);
        if item.trigger_kind == "file_change" {
            if let Some(path) = automations::watch_path(&item.trigger) {
                let path = PathBuf::from(path);
                let scope = if path.is_dir() { WatchScope::Folder { recursive: true } } else { WatchScope::File };
                watches.push(WatchTarget { connection_id: item.id.clone(), path, scope });
            }
        }
    }
    if !watches.is_empty() {
        let handle = app.clone();
        match watcher::start(watches, move |ids| {
            for id in ids {
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let Some(state) = app.try_state::<AppState>() else { return };
                    if run_one(&app, &state, &id, "file_change").await.is_ok() {
                        let _ = app.emit(AUTOMATION_EVENT, json!({ "id": id }));
                    }
                });
            }
        }) {
            Ok(watcher) => {
                if let Err(error) = state.set_file_jobs(Some(watcher)) {
                    tracing::warn!("automation file watch: {error}");
                }
            }
            Err(error) => tracing::warn!("automation file watch: {error}"),
        }
    }
}

fn schedule(state: &AppState, app: &AppHandle, item: &AutomationRecord) {
    state.jobs.stop(&item.id);
    if !item.enabled || item.needs_review {
        return;
    }
    let app = app.clone();
    let id = item.id.clone();
    match item.trigger_kind.as_str() {
        "interval" => {
            let Some(secs) = automations::interval_secs(&item.trigger) else { return };
            let handle = tauri::async_runtime::spawn({
                let id = id.clone();
                async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(secs as u64));
                    interval.tick().await;
                    loop {
                        interval.tick().await;
                        let Some(state) = app.try_state::<AppState>() else { break };
                        if run_one(&app, &state, &id, "interval").await.is_ok() {
                            let _ = app.emit(AUTOMATION_EVENT, json!({ "id": id }));
                        }
                    }
                }
            });
            state.jobs.track(id, handle);
        }
        "cron" => {
            let Some(expr) = automations::cron_expr(&item.trigger).map(str::to_owned) else { return };
            let handle = tauri::async_runtime::spawn({
                let id = id.clone();
                async move {
                    loop {
                        let wait = match automations::next_cron(&expr) {
                            Ok(wait) => wait,
                            Err(error) => {
                                tracing::warn!("cron {id}: {error}");
                                break;
                            }
                        };
                        tokio::time::sleep(wait).await;
                        let Some(state) = app.try_state::<AppState>() else { break };
                        if run_one(&app, &state, &id, "cron").await.is_ok() {
                            let _ = app.emit(AUTOMATION_EVENT, json!({ "id": id }));
                        }
                    }
                }
            });
            state.jobs.track(id, handle);
        }
        _ => {}
    }
}

pub async fn run_one(app: &AppHandle, state: &AppState, id: &str, trigger_kind: &str) -> AppResult<()> {
    let workspace = state.workspace()?;
    let automation = blocking({
        let workspace = workspace.clone();
        let id = id.to_owned();
        move || workspace.db()?.automation(&id)
    })
    .await?;
    if automation.needs_review {
        return Err(AppError::invalid("Review this automation before it can run"));
    }
    let run_id = new_id();
    let started = Instant::now();
    blocking({
        let workspace = workspace.clone();
        let automation_id = automation.id.clone();
        let run_id = run_id.clone();
        let trigger_kind = trigger_kind.to_owned();
        move || workspace.db()?.start_automation_run(&run_id, &automation_id, &trigger_kind)
    })
    .await?;

    let previous = blocking({
        let workspace = workspace.clone();
        let automation_id = automation.id.clone();
        move || workspace.db()?.last_ok_facts(&automation_id)
    })
    .await
    .ok()
    .flatten();

    let mut last_facts: Option<Facts> = None;
    let mut last_result_hash = String::new();
    let mut last_alert: Option<automations::Alert> = None;
    let mut summaries = Vec::new();
    let mut all_relations = Vec::new();
    let mut failed = None;
    let mut last_sql: Option<String> = None;
    let mut last_question: Option<String> = None;
    let mut last_rows: Option<i64> = None;
    let mut last_data: Option<(Vec<String>, Vec<Vec<Value>>)> = None;
    let mut stopped = false;

    for step in &automation.steps {
        let step_run = new_id();
        blocking({
            let workspace = workspace.clone();
            let run_id = run_id.clone();
            let step_id = step.id.clone();
            let kind = step.kind.clone();
            let sort = step.sort;
            let step_run = step_run.clone();
            move || workspace.db()?.start_step_run(&step_run, &run_id, &step_id, sort, &kind)
        })
        .await?;

        let outcome = match step.kind.as_str() {
            "sql" => run_sql(state, step).await,
            "ask" => run_ask(state, step).await,
            "dashboard" => run_dashboard(state, step).await,
            "compare" => match last_facts.as_ref() {
                Some(facts) => {
                    let column = step.config.get("column").and_then(Value::as_str).map(str::to_owned);
                    let alert = automations::compare_alert(facts, previous.as_ref(), column.as_deref());
                    Ok(StepOut { summary: alert.text.clone(), hash: alert.hash.clone(), rows: None, relations: Vec::new(), facts: Some(facts.clone()), alert: Some(alert), sql: None, question: None, data: None, stop: false, output: None })
                }
                None => Err(step_err("Compare needs a SQL step before it")),
            },
            "threshold" => match last_facts.as_ref() {
                Some(facts) => match serde_json::from_value::<Threshold>(step.config.clone()) {
                    Ok(spec) => {
                        let alert = automations::threshold_alert(facts, &spec);
                        Ok(StepOut { summary: alert.text.clone(), hash: alert.hash.clone(), rows: None, relations: Vec::new(), facts: Some(facts.clone()), alert: Some(alert), sql: None, question: None, data: None, stop: false, output: None })
                    }
                    Err(_) => Err(step_err("Threshold needs an op and a numeric value")),
                },
                None => Err(step_err("Threshold needs a SQL step before it")),
            },
            "anomaly" => match last_facts.as_ref() {
                Some(facts) => {
                    let alert = automations::anomaly_alert(facts);
                    Ok(StepOut { summary: alert.text.clone(), hash: alert.hash.clone(), rows: None, relations: Vec::new(), facts: Some(facts.clone()), alert: Some(alert), sql: None, question: None, data: None, stop: false, output: None })
                }
                None => Err(step_err("Anomaly check needs a SQL step before it")),
            },
            "report" => run_report(state, step).await,
            "email" => {
                let body = last_alert.as_ref().map(|alert| alert.text.as_str()).or(summaries.last().map(String::as_str)).unwrap_or("Automation finished");
                send_email(state, step, &automation.name, body).await
            }
            "mcp_tool" => run_mcp_tool(state, step, &automation.id).await,
            "webhook" => run_webhook(state, step, &automation.name, &summaries, last_facts.as_ref(), last_data.as_ref()).await,
            "write_file" => run_write_file(step, &automation.name, last_data.as_ref()),
            "notify" => {
                let title = step.config.get("title").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).unwrap_or(&automation.name).to_owned();
                let body = step
                    .config
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
                    .or_else(|| last_alert.as_ref().map(|alert| alert.text.clone()))
                    .or_else(|| summaries.last().cloned())
                    .unwrap_or_else(|| "Automation finished".into());
                notify(app, &title, &body);
                let summary = format!("Notified: {body}");
                Ok(StepOut { hash: retrieval::content_hash(&summary), summary, rows: None, relations: Vec::new(), facts: None, alert: None, sql: None, question: None, data: None, stop: false, output: Some(json!({ "input": { "title": title, "message": body }, "output": "shown" })) })
            }
            "branch" => run_branch(step, last_alert.as_ref(), last_rows, last_facts.as_ref()),
            other => Err(step_err(format!("Unknown step kind: {other}"))),
        };

        match outcome {
            Ok(out) => {
                if let Some(facts) = out.facts.clone() {
                    last_facts = Some(facts);
                }
                if !out.hash.is_empty() {
                    last_result_hash = out.hash.clone();
                }
                if let Some(alert) = out.alert.clone() {
                    last_alert = Some(alert);
                }
                if let Some(sql) = out.sql.clone() {
                    last_sql = Some(sql);
                }
                if let Some(question) = out.question.clone() {
                    last_question = Some(question);
                }
                take_rows(&out, &mut last_rows);
                if out.data.is_some() {
                    last_data = out.data.clone();
                }
                if out.stop {
                    stopped = true;
                }
                all_relations.extend(out.relations);
                summaries.push(out.summary.clone());
                let facts_json = out.output.clone().unwrap_or_else(|| last_alert.as_ref().map(|alert| alert.facts.clone()).unwrap_or(json!({ "summary": out.summary })));
                let _ = blocking({
                    let workspace = workspace.clone();
                    let step_run = step_run.clone();
                    let summary = out.summary.clone();
                    move || workspace.db()?.finish_step_run(&step_run, "ok", &summary, &facts_json)
                })
                .await;
            }
            Err((denied, error, _)) => {
                let _ = blocking({
                    let workspace = workspace.clone();
                    let step_run = step_run.clone();
                    let error = error.clone();
                    move || workspace.db()?.finish_step_run(&step_run, "error", &error, &json!({ "error": error }))
                })
                .await;
                failed = Some((denied, error));
                break;
            }
        }
        if stopped {
            break;
        }
    }

    let (status, summary, hash, audit_status) = match failed {
        None => {
            let summary = if stopped {
                summaries.last().cloned().unwrap_or_else(|| "Stopped".into())
            } else {
                last_alert.as_ref().map(|alert| alert.text.clone()).or_else(|| summaries.last().cloned()).unwrap_or_else(|| "Finished".into())
            };
            let hash = last_alert.as_ref().map(|alert| alert.hash.clone()).unwrap_or(last_result_hash);
            ("ok".to_owned(), summary, hash, AuditStatus::Ok)
        }
        Some((denied, error)) => ("error".to_owned(), error, String::new(), if denied { AuditStatus::Denied } else { AuditStatus::Error }),
    };

    let duration = elapsed_ms(started);
    if let Ok(db) = workspace.db() {
        db.try_record_audit_lineage(
            &AuditEntry {
                kind: "automation.run",
                status: audit_status,
                duration_ms: Some(duration),
                query: AuditQuery { question: last_question.as_deref(), sql: last_sql.as_deref(), rows: last_rows },
                detail: json!({
                    "automation": automation.id,
                    "name": automation.name,
                    "trigger": trigger_kind,
                    "run": run_id,
                    "summary": summary,
                }),
            },
            &all_relations,
        );
    }

    let changed = !automation.on_change_only || automation.last_hash.as_deref() != Some(hash.as_str());
    let interesting = last_alert.as_ref().is_none_or(|alert| alert.fired);
    let should_notify = status == "error" || (!stopped && changed && interesting && !summary.is_empty());
    if should_notify {
        let body = if status == "ok" {
            match last_alert.as_ref() {
                Some(alert) => narrate_alert(state, &automation.name, &alert.facts).await.filter(|text| !text.is_empty()).unwrap_or_else(|| summary.clone()),
                None => summary.clone(),
            }
        } else {
            summary.clone()
        };
        notify(app, &automation.name, &body);
    }
    blocking({
        let workspace = workspace.clone();
        let automation_id = automation.id.clone();
        move || workspace.db()?.finish_automation_run(&automation_id, &run_id, &status, &summary, &hash, should_notify)
    })
    .await?;
    Ok(())
}

struct StepOut {
    summary: String,
    hash: String,
    rows: Option<i64>,
    relations: Vec<String>,
    facts: Option<Facts>,
    alert: Option<automations::Alert>,
    sql: Option<String>,
    question: Option<String>,
    /// Columns and rows of the latest result, for webhook and file steps.
    data: Option<(Vec<String>, Vec<Vec<Value>>)>,
    /// Stop the run here (a branch condition was not met).
    stop: bool,
    /// Stored as the step's output when it is richer than the summary.
    output: Option<Value>,
}

fn take_rows(out: &StepOut, last_rows: &mut Option<i64>) {
    if out.rows.is_some() {
        *last_rows = out.rows;
    }
}

fn step_err(message: impl ToString) -> (bool, String, Option<i64>) {
    (false, message.to_string(), None)
}

fn step_denied(message: impl ToString) -> (bool, String, Option<i64>) {
    (true, message.to_string(), None)
}

async fn run_sql(state: &AppState, step: &AutomationStep) -> Result<StepOut, (bool, String, Option<i64>)> {
    let sql = step
        .config
        .get("sql")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| step_err("This step has no SQL"))?
        .to_owned();
    let workspace = state.workspace().map_err(step_err)?;
    let outcome = blocking({
        let sql = sql.clone();
        move || {
            let policy = GuardPolicy::new(workspace.db()?.list_tables()?.into_iter().map(|table| table.relation));
            let validated = match sql_guard::validate(&sql, &policy) {
                Ok(validated) => validated,
                Err(error) => return Ok(Err(step_denied(error))),
            };
            let relations = validated.relations.clone();
            let bound = bind_params(&validated.parameters, &serde_json::Map::new()).map_err(AppError::invalid)?;
            let result = query::run(&workspace.service_connection()?, &validated, &bound, &CancelToken::default())
                .map_err(|error| AppError::invalid(error.to_string()))?;
            let alert = automations::sql_alert(&result);
            let facts = facts::compute(&result);
            Ok(Ok(StepOut {
                summary: alert.text.clone(),
                hash: alert.hash.clone(),
                rows: Some(result.rows.len() as i64),
                relations,
                facts: Some(facts),
                alert: Some(alert),
                sql: Some(validated.sql),
                question: None,
                data: Some((result.columns.iter().map(|column| column.name.clone()).collect(), result.rows.clone())),
                stop: false,
                output: None,
            }))
        }
    })
    .await
    .map_err(step_err)?;
    outcome
}

async fn run_ask(state: &AppState, step: &AutomationStep) -> Result<StepOut, (bool, String, Option<i64>)> {
    let question = step
        .config
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| step_err("This step has no question"))?
        .to_owned();
    let workspace = state.workspace().map_err(step_err)?;
    let routing: TaskRouting = state.app_db().map_err(step_err)?.setting(SETTINGS_KEY).map_err(step_err)?.unwrap_or_default();
    let models = Models::available(state.llm.clone(), routing);
    let (user_id, assistant_id, conversation_id) = (new_id(), new_id(), new_id());
    let stored_question = question.clone();
    blocking({
        let workspace = workspace.clone();
        let conversation_id = conversation_id.clone();
        let assistant_id = assistant_id.clone();
        move || {
            let db = workspace.db()?;
            db.create_conversation(&conversation_id, &title_from(&stored_question))?;
            db.insert_message(&user_id, &conversation_id, MessageRole::User, &stored_question, MessageStatus::Done)?;
            db.insert_message(&assistant_id, &conversation_id, MessageRole::Assistant, "", MessageStatus::Pending)?;
            Ok::<_, AppError>(())
        }
    })
    .await
    .map_err(step_err)?;
    let cancel = state.analyst_runs.begin(&assistant_id).map_err(step_err)?;
    let sink: EventSink = Arc::new(|_: AnalystEvent| {});
    let run = Run::new(workspace, models, cancel, sink, conversation_id, assistant_id.clone(), question.clone());
    let text = run.execute().await;
    state.analyst_runs.finish(&assistant_id);
    let hash = retrieval::content_hash(&text);
    Ok(StepOut {
        summary: text.chars().take(240).collect(),
        hash,
        rows: None,
        relations: Vec::new(),
        facts: None,
        alert: None,
        sql: None,
        question: Some(question),
        data: None,
        stop: false,
        output: None,
    })
}

async fn run_dashboard(state: &AppState, step: &AutomationStep) -> Result<StepOut, (bool, String, Option<i64>)> {
    let dashboard_id = step.config.get("dashboardId").and_then(Value::as_str).ok_or_else(|| step_err("This step has no dashboard"))?.to_owned();
    let workspace = state.workspace().map_err(step_err)?;
    let (dashboard, raw) = blocking({
        let workspace = workspace.clone();
        let dashboard_id = dashboard_id.clone();
        move || {
            let db = workspace.db()?;
            let params = db.last_params(&dashboard_id)?;
            Ok((db.dashboard(&dashboard_id)?, serde_json::from_str(&params).unwrap_or_default()))
        }
    })
    .await
    .map_err(step_err)?;
    let tiles = crate::commands::dashboards::live_tiles(state, &workspace, &dashboard_id, dashboard.tiles, &raw).await.map_err(step_err)?;
    let insights: Vec<String> = tiles.iter().filter_map(|tile| tile.tile.insight.clone()).collect();
    let errors: Vec<String> = tiles.iter().filter_map(|tile| tile.error.clone()).collect();
    let summary = if !errors.is_empty() {
        format!("{} · {} tiles failed", dashboard.name, errors.len())
    } else if insights.is_empty() {
        format!("{} refreshed", dashboard.name)
    } else {
        insights.into_iter().take(2).collect::<Vec<_>>().join(" ")
    };
    let hash = retrieval::content_hash(&summary);
    Ok(StepOut { summary, hash, rows: None, relations: Vec::new(), facts: None, alert: None, sql: None, question: None, data: None, stop: false, output: None })
}

async fn run_report(state: &AppState, step: &AutomationStep) -> Result<StepOut, (bool, String, Option<i64>)> {
    let prompt = step
        .config
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| step_err("This step has no report prompt"))?
        .to_owned();
    let kind = step.config.get("artifact").and_then(Value::as_str).unwrap_or("pdf");
    let summary = match kind {
        "deck" => {
            let generated = generate_deck(state, prompt, None).await.map_err(step_err)?;
            format!("Presentation · {}", generated.deck.name)
        }
        _ => {
            let generated = generate_report(state, prompt, None).await.map_err(step_err)?;
            format!("Report · {}", generated.report.name)
        }
    };
    Ok(StepOut { hash: retrieval::content_hash(&summary), summary, rows: None, relations: Vec::new(), facts: None, alert: None, sql: None, question: None, data: None, stop: false, output: None })
}

async fn send_email(state: &AppState, step: &AutomationStep, subject: &str, body: &str) -> Result<StepOut, (bool, String, Option<i64>)> {
    let workspace = state.workspace().map_err(step_err)?;
    let workspace_id = workspace.id.clone();
    let settings = blocking({
        let workspace = workspace.clone();
        move || Ok(workspace.db()?.workspace_setting::<SmtpStored>(SMTP_KEY)?.unwrap_or_default())
    })
    .await
    .map_err(step_err)?;
    if !settings.enabled {
        return Err(step_err("Email is off. Turn on SMTP in Settings before this step can send."));
    }
    if state.is_offline() {
        return Err(step_err("Offline mode is on. Email would leave this device."));
    }
    let to = step
        .config
        .get("to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| step_err("This email step has no recipient"))?
        .to_owned();
    let password = blocking({
        let reference = smtp_secret(&workspace_id);
        move || secrets::get(&reference)
    })
    .await
    .map_err(|_| step_err("SMTP password is not in the keychain"))?;
    deliver_smtp(&settings, &password, &to, subject, body).await.map_err(step_err)?;
    let summary = format!("Emailed {to}");
    Ok(StepOut { hash: retrieval::content_hash(&summary), summary, rows: None, relations: Vec::new(), facts: None, alert: None, sql: None, question: None, data: None, stop: false, output: None })
}

#[derive(Clone, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SmtpStored {
    enabled: bool,
    host: String,
    port: u16,
    username: String,
    from: String,
}

async fn deliver_smtp(settings: &SmtpStored, password: &str, to: &str, subject: &str, body: &str) -> Result<(), String> {
    use lettre::message::Mailbox;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let from: Mailbox = settings.from.parse().map_err(|_| "SMTP from address is not valid".to_owned())?;
    let to: Mailbox = to.parse().map_err(|_| "Email recipient is not valid".to_owned())?;
    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .body(body.to_owned())
        .map_err(|error| error.to_string())?;
    let creds = Credentials::new(settings.username.clone(), password.to_owned());
    let transport = if settings.port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.host).map_err(|error| error.to_string())?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.host).map_err(|error| error.to_string())?
    }
    .port(settings.port)
    .credentials(creds)
    .build();
    transport.send(message).await.map_err(|error| error.to_string())?;
    Ok(())
}

async fn narrate_alert(state: &AppState, title: &str, facts: &Value) -> Option<String> {
    use crate::engine::analyst::prompts;
    let routing: TaskRouting = state.app_db().ok()?.setting(SETTINGS_KEY).ok().flatten().unwrap_or_default();
    let spec = state.llm.resolve(&routing, Task::Narrative)?;
    let provider = state.llm.provider(spec.id).await.ok()?;
    let response = provider
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
        .ok()?;
    Some(response.content.trim().to_owned())
}

fn attach_lineage(workspace: &crate::state::Workspace, id: &str, steps: &[AutomationStep]) -> AppResult<()> {
    let db = workspace.db()?;
    let policy = GuardPolicy::new(db.list_tables()?.into_iter().map(|table| table.relation));
    let mut relations = Vec::new();
    for step in steps {
        if step.kind == "sql" {
            if let Some(sql) = step.config.get("sql").and_then(Value::as_str) {
                if let Ok(validated) = sql_guard::validate(sql, &policy) {
                    relations.extend(validated.relations);
                }
            }
        }
    }
    if relations.is_empty() {
        db.clear_query_relations("automation", id)
    } else {
        db.replace_query_relations("automation", id, &relations)
    }
}

#[tauri::command]
pub async fn automation_list(state: State<'_, AppState>) -> AppResult<Vec<AutomationRecord>> {
    let workspace = state.workspace()?;
    let origin = state.server_origin();
    blocking(move || {
        Ok(workspace.db()?.list_automations()?.into_iter().map(|item| with_webhook(item, origin.as_deref())).collect())
    })
    .await
}

#[tauri::command]
pub async fn automation_save(app: AppHandle, state: State<'_, AppState>, draft: AutomationDraft) -> AppResult<AutomationRecord> {
    state.ensure_unlocked()?;
    let name = draft.name.trim().to_owned();
    if name.is_empty() {
        return Err(AppError::invalid("Name the job"));
    }
    let trigger = validate_trigger(&draft.trigger_kind, &draft.trigger)?;
    let steps = validate_steps(&draft.steps)?;
    let workspace = state.workspace()?;
    let origin = state.server_origin();
    let id = draft.id.clone().unwrap_or_else(new_id);
    let existing = if draft.id.is_some() {
        blocking({
            let workspace = workspace.clone();
            let id = id.clone();
            move || workspace.db()?.automation(&id)
        })
        .await
        .ok()
    } else {
        None
    };
    let source = existing.as_ref().map(|item| item.source.clone()).unwrap_or_else(|| "user".into());
    let needs_review = existing.as_ref().map(|item| item.needs_review).unwrap_or(false);
    let confirmed_at = existing.as_ref().and_then(|item| item.confirmed_at.clone());
    if draft.enabled && needs_review {
        return Err(AppError::invalid("Review this automation before enabling it"));
    }
    let mut webhook_hash = None;
    let mut issued_token = None;
    if draft.trigger_kind == "webhook" && existing.as_ref().is_none_or(|item| !item.has_webhook) {
        let token = new_webhook_token();
        webhook_hash = Some(hash_token(&token));
        issued_token = Some(token.clone());
        blocking({
            let reference = format!("webhook:{id}");
            move || secrets::save(&reference, &token)
        })
        .await?;
    }
    let record = AutomationRecord {
        id: id.clone(),
        name,
        trigger_kind: draft.trigger_kind,
        trigger,
        steps,
        enabled: draft.enabled && !needs_review,
        on_change_only: draft.on_change_only,
        needs_review,
        source,
        confirmed_at,
        last_run_at: None,
        last_status: None,
        last_summary: None,
        last_hash: None,
        has_webhook: webhook_hash.is_some() || existing.as_ref().is_some_and(|item| item.has_webhook),
        webhook_url: None,
        webhook_token: None,
    };
    let saved = blocking({
        let workspace = workspace.clone();
        let record = record.clone();
        let webhook_hash = webhook_hash.clone();
        move || {
            let saved = workspace.db()?.upsert_automation(&record, webhook_hash.as_deref())?;
            attach_lineage(&workspace, &saved.id, &saved.steps)?;
            Ok(saved)
        }
    })
    .await?;
    resume(&app);
    let mut saved = with_webhook(saved, origin.as_deref());
    saved.webhook_token = issued_token;
    Ok(saved)
}

#[tauri::command]
pub async fn automation_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    state.jobs.stop(&id);
    let workspace = state.workspace()?;
    blocking({
        let id = id.clone();
        move || {
            let db = workspace.db()?;
            db.clear_query_relations("automation", &id)?;
            let _ = secrets::delete(&format!("webhook:{id}"));
            db.delete_automation(&id)
        }
    })
    .await?;
    resume(&app);
    Ok(())
}

#[tauri::command]
pub async fn automation_runs(state: State<'_, AppState>, id: String) -> AppResult<Vec<AutomationRun>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.automation_runs(&id)).await
}

#[tauri::command]
pub async fn automation_run(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    run_one(&app, &state, &id, "manual").await?;
    let _ = app.emit(AUTOMATION_EVENT, json!({ "id": id }));
    Ok(())
}

#[tauri::command]
pub async fn automation_confirm(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<AutomationRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let origin = state.server_origin();
    let saved = blocking(move || workspace.db()?.confirm_automation(&id)).await?;
    resume(&app);
    Ok(with_webhook(saved, origin.as_deref()))
}

#[tauri::command]
pub async fn automation_rotate_webhook(state: State<'_, AppState>, id: String) -> AppResult<WebhookCreated> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let origin = state.server_origin().ok_or_else(|| AppError::invalid("The local site server is not running"))?;
    let token = new_webhook_token();
    let hash = hash_token(&token);
    blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        let token = token.clone();
        move || {
            workspace.db()?.set_webhook_hash(&id, &hash)?;
            secrets::save(&format!("webhook:{id}"), &token)
        }
    })
    .await?;
    Ok(WebhookCreated { url: webhook_url(&origin, &id), token })
}

/** Plain words for a step kind, shown while a draft is being written. */
fn step_word(kind: &str) -> &'static str {
    match kind {
        "sql" => "Run SQL",
        "ask" => "Ask the analyst",
        "dashboard" => "Refresh a dashboard",
        "compare" => "Compare with last run",
        "threshold" => "Check a threshold",
        "anomaly" => "Spot anomalies",
        "report" => "Write a report",
        "email" => "Send an email",
        "notify" => "Notify",
        "branch" => "Branch",
        _ => "Step",
    }
}

/** The value of the first complete `"field": "..."` string in partial JSON. */
fn partial_string<'a>(text: &'a str, field: &str) -> Option<&'a str> {
    let key = format!("\"{field}\"");
    let rest = &text[text.find(&key)? + key.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

/** What the model is writing right now, read from its partial JSON. */
fn draft_progress(text: &str) -> Option<String> {
    let kinds: Vec<&str> = text
        .match_indices("\"kind\"")
        .filter_map(|(index, _)| partial_string(&text[index..], "kind"))
        .collect();
    if let Some(last) = kinds.last() {
        return Some(format!("Step {}: {}", kinds.len(), step_word(last)));
    }
    if text.contains("\"steps\"") {
        return Some("Planning the steps".into());
    }
    if let Some(kind) = partial_string(text, "triggerKind") {
        return Some(format!("Runs {}", trigger_kind_words(kind)));
    }
    partial_string(text, "name").map(|name| format!("Naming it “{}”", name.trim()))
}

fn trigger_kind_words(kind: &str) -> &'static str {
    match kind {
        "cron" => "on a schedule",
        "interval" => "every so often",
        "file_change" => "when a file changes",
        "webhook" => "from a webhook",
        _ => "when you start it",
    }
}

fn trigger_words(kind: &str, trigger: &Value) -> String {
    match kind {
        "cron" => format!("Schedule {}", trigger.get("cron").and_then(Value::as_str).unwrap_or("")),
        "interval" => format!("Every {} min", trigger.get("intervalSecs").and_then(Value::as_u64).unwrap_or(0) / 60),
        "file_change" => format!("When {} changes", trigger.get("path").and_then(Value::as_str).unwrap_or("the file")),
        other => format!("Runs {}", trigger_kind_words(other)),
    }
}

#[tauri::command]
pub async fn automation_propose(app: AppHandle, state: State<'_, AppState>, prompt: String) -> AppResult<AutomationRecord> {
    state.ensure_unlocked()?;
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(AppError::invalid("Describe the automation"));
    }
    let workspace = state.workspace()?;
    let app = Some(&app);
    crate::commands::progress(app, "context", "Finding the relevant tables", StepStatus::Running, None);
    let context = blocking({
        let workspace = workspace.clone();
        let prompt = prompt.clone();
        move || crate::engine::analyst::context::build(&workspace, &prompt, &[])
    })
    .await?;
    let found = context.tables.iter().map(|table| table.relation.as_str()).collect::<Vec<_>>().join(", ");
    crate::commands::progress(
        app,
        "context",
        "Finding the relevant tables",
        StepStatus::Done,
        Some(if found.is_empty() { "No tables yet".into() } else { found }),
    );
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let spec = state.llm.resolve(&routing, Task::Sql).ok_or(crate::llm::provider::LlmError::NoModel)?;
    crate::commands::progress(app, "model", format!("Starting {}", spec.id), StepStatus::Running, None);
    let provider = state.llm.provider(spec.id).await?;
    crate::commands::progress(app, "model", format!("Starting {}", spec.id), StepStatus::Done, None);
    crate::commands::progress(app, "design", "Designing the automation", StepStatus::Running, None);
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "triggerKind": { "type": "string", "enum": ["cron", "interval", "file_change", "manual", "webhook"] },
            "trigger": {
                "type": "object",
                "properties": {
                    "cron": { "type": "string" },
                    "intervalSecs": { "type": "integer" },
                    "path": { "type": "string" }
                }
            },
            "steps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["sql", "ask", "dashboard", "compare", "threshold", "anomaly", "report", "email", "notify", "branch"] },
                        "config": { "type": "object" }
                    },
                    "required": ["kind", "config"]
                }
            }
        },
        "required": ["name", "triggerKind", "trigger", "steps"]
    });
    let (tokens, mut received) = tokio::sync::mpsc::channel::<String>(64);
    // Say what the model is writing as it streams, so a slow local model does not look stuck.
    let forward = async {
        let mut text = String::new();
        let mut shown = String::new();
        while let Some(delta) = received.recv().await {
            text.push_str(&delta);
            if let Some(detail) = draft_progress(&text) {
                if detail != shown {
                    crate::commands::progress(app, "design", "Designing the automation", StepStatus::Running, Some(detail.clone()));
                    shown = detail;
                }
            }
        }
    };
    let (response, ()) = tokio::join!(
        provider.chat(
            ChatRequest {
                messages: vec![
                    ChatMessage {
                        role: Role::System,
                        content: "Propose a LocalBoard automation. Use only read-only SELECT SQL over existing workspace tables. \
                         Numbers in thresholds must come from the user. Do not invent tables.\n\
                         Trigger shapes, by triggerKind:\n\
                         - cron: {\"cron\": \"<minute> <hour> <day of month> <month> <day of week>\"}. \
                         Every morning at 9:00 is \"0 9 * * *\"; every Monday at 9:00 is \"0 9 * * 1\"; \
                         the first of each month at 8:00 is \"0 8 1 * *\".\n\
                         - interval: {\"intervalSecs\": 3600} (at least 60).\n\
                         - file_change: {\"path\": \"<file or folder>\"}.\n\
                         - manual or webhook: {}.\n\
                         If the user names no time, use manual.\n\
                         Tables you can use:\n"
                            .to_owned()
                            + &context.prompt,
                    },
                    ChatMessage { role: Role::User, content: prompt },
                ],
                temperature: 0.2,
                max_tokens: 1_024,
                format: ResponseFormat::JsonSchema { name: "automation".into(), schema },
            },
            Some(tokens),
        ),
        forward
    );
    let response = response?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Proposed {
        name: String,
        trigger_kind: String,
        trigger: Value,
        steps: Vec<StepDraft>,
    }
    let proposed: Proposed = serde_json::from_str(response.content.trim()).map_err(|_| AppError::invalid("The model returned an unreadable automation"))?;
    crate::commands::progress(app, "design", "Designing the automation", StepStatus::Done, Some(format!("{} · {} steps", proposed.name.trim(), proposed.steps.len())));
    crate::commands::progress(app, "trigger", "Checking when it runs", StepStatus::Running, None);
    // A draft is reviewed before it runs, so an unusable trigger becomes "manual" instead of losing the whole draft.
    let (trigger_kind, trigger) = match validate_trigger(&proposed.trigger_kind, &proposed.trigger) {
        Ok(trigger) => {
            crate::commands::progress(app, "trigger", "Checking when it runs", StepStatus::Done, Some(trigger_words(&proposed.trigger_kind, &trigger)));
            (proposed.trigger_kind.clone(), trigger)
        }
        Err(_) => {
            crate::commands::progress(app, "trigger", "Checking when it runs", StepStatus::Done, Some("The schedule was unclear, so it runs manually. Set it in the editor.".into()));
            ("manual".to_owned(), json!({}))
        }
    };
    crate::commands::progress(app, "steps", "Checking the steps", StepStatus::Running, None);
    let steps = match validate_steps(&proposed.steps) {
        Ok(steps) => steps,
        Err(error) => {
            crate::commands::progress(app, "steps", "Checking the steps", StepStatus::Error, Some(error.to_string()));
            return Err(error);
        }
    };
    let words = steps.iter().map(|step| step_word(&step.kind)).collect::<Vec<_>>().join(" → ");
    crate::commands::progress(app, "steps", "Checking the steps", StepStatus::Done, Some(words));
    crate::commands::progress(app, "save", "Saving the draft", StepStatus::Running, None);
    let origin = state.server_origin();
    let record = AutomationRecord {
        id: new_id(),
        name: proposed.name.trim().to_owned(),
        trigger_kind,
        trigger,
        steps,
        enabled: false,
        on_change_only: true,
        needs_review: true,
        source: "ai".into(),
        confirmed_at: None,
        last_run_at: None,
        last_status: None,
        last_summary: None,
        last_hash: None,
        has_webhook: false,
        webhook_url: None,
        webhook_token: None,
    };
    if record.name.is_empty() {
        return Err(AppError::invalid("The model did not name the automation"));
    }
    let saved = blocking({
        let workspace = workspace.clone();
        let record = record.clone();
        move || {
            let saved = workspace.db()?.upsert_automation(&record, None)?;
            attach_lineage(&workspace, &saved.id, &saved.steps)?;
            Ok(saved)
        }
    })
    .await?;
    crate::commands::progress(app, "save", "Saving the draft", StepStatus::Done, Some("Review it, then turn it on".into()));
    Ok(with_webhook(saved, origin.as_deref()))
}

#[tauri::command]
pub async fn smtp_get(state: State<'_, AppState>) -> AppResult<SmtpSettings> {
    let workspace = state.workspace()?;
    let workspace_id = workspace.id.clone();
    blocking(move || {
        let stored = workspace.db()?.workspace_setting::<SmtpStored>(SMTP_KEY)?.unwrap_or_default();
        let has_password = secrets::get(&smtp_secret(&workspace_id)).is_ok();
        Ok(SmtpSettings {
            enabled: stored.enabled,
            host: stored.host,
            port: if stored.port == 0 { 587 } else { stored.port },
            username: stored.username,
            from: stored.from,
            has_password,
        })
    })
    .await
}

#[tauri::command]
pub async fn smtp_save(state: State<'_, AppState>, draft: SmtpDraft) -> AppResult<SmtpSettings> {
    state.ensure_unlocked()?;
    if draft.enabled {
        state.ensure_online("SMTP")?;
    }
    let workspace = state.workspace()?;
    let workspace_id = workspace.id.clone();
    blocking(move || {
        if let Some(password) = draft.password.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
            secrets::save(&smtp_secret(&workspace_id), password)?;
        }
        let stored = SmtpStored { enabled: draft.enabled, host: draft.host.trim().to_owned(), port: draft.port, username: draft.username.trim().to_owned(), from: draft.from.trim().to_owned() };
        if stored.enabled && (stored.host.is_empty() || stored.from.is_empty()) {
            return Err(AppError::invalid("SMTP host and from address are required when email is on"));
        }
        workspace.db()?.set_workspace_setting(SMTP_KEY, &stored)?;
        Ok(SmtpSettings {
            enabled: stored.enabled,
            host: stored.host,
            port: stored.port,
            username: stored.username,
            from: stored.from,
            has_password: secrets::get(&smtp_secret(&workspace_id)).is_ok(),
        })
    })
    .await
}

type StepResult = Result<StepOut, (bool, String, Option<i64>)>;

fn plain_out(summary: String, output: Value) -> StepOut {
    StepOut { hash: retrieval::content_hash(&summary), summary, rows: None, relations: Vec::new(), facts: None, alert: None, sql: None, question: None, data: None, stop: false, output: Some(output) }
}

fn cfg_str<'a>(step: &'a AutomationStep, key: &str) -> Option<&'a str> {
    step.config.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

/// Agent id used for MCP tool rules when an automation calls a tool.
const MCP_AUTOMATION_AGENT: &str = "automations";

async fn run_mcp_tool(state: &AppState, step: &AutomationStep, automation_id: &str) -> StepResult {
    use crate::engine::mcp;
    let server_id = cfg_str(step, "serverId").ok_or_else(|| step_err("Choose an MCP server"))?.to_owned();
    let tool = cfg_str(step, "tool").ok_or_else(|| step_err("Name the MCP tool"))?.to_owned();
    let args = match step.config.get("args") {
        Some(Value::String(text)) if !text.trim().is_empty() => serde_json::from_str::<Value>(text).map_err(|_| step_err("Tool arguments must be valid JSON"))?,
        Some(value @ Value::Object(_)) => value.clone(),
        _ => json!({}),
    };
    let workspace = state.workspace().map_err(step_err)?;
    let (server, rule) = blocking({
        let workspace = workspace.clone();
        let server_id = server_id.clone();
        let tool = tool.clone();
        move || {
            let db = workspace.db()?;
            let server = db.mcp_server(&server_id)?;
            let rule = db.mcp_rule(MCP_AUTOMATION_AGENT, &server_id, &tool)?;
            Ok((server, rule))
        }
    })
    .await
    .map_err(step_err)?;
    if !server.enabled {
        return Err(step_denied(format!("MCP server {} is turned off", server.name)));
    }
    let allowed = match rule {
        Some(rule) => rule.allow,
        None => !mcp::is_write_like(&tool),
    };
    if !allowed {
        mcp::audit_tool(&workspace, &server.name, &tool, false, AuditStatus::Denied);
        return Err(step_denied(format!("{tool} is not allowed for automations. Allow it for the \"{MCP_AUTOMATION_AGENT}\" agent in MCP settings.")));
    }
    let read_only = !mcp::is_write_like(&tool);
    match mcp::call_remote(&server, "tools/call", json!({ "name": tool, "arguments": args }), state.is_offline()).await {
        Ok(result) => {
            mcp::audit_tool(&workspace, &server.name, &tool, read_only, AuditStatus::Ok);
            let text = mcp::wrap_untrusted(&result.to_string());
            let _ = automation_id;
            let summary: String = format!("Called {} / {tool}", server.name);
            let clipped: String = text.chars().take(4_000).collect();
            Ok(plain_out(summary, json!({ "input": { "server": server.name, "tool": tool, "args": args }, "output": clipped })))
        }
        Err(error) => {
            mcp::audit_tool(&workspace, &server.name, &tool, read_only, AuditStatus::Error);
            Err(step_err(error))
        }
    }
}

pub fn webhook_secret_ref(workspace_id: &str, step_id: &str) -> String {
    format!("stephook:{workspace_id}:{step_id}")
}

async fn run_webhook(state: &AppState, step: &AutomationStep, name: &str, summaries: &[String], facts: Option<&Facts>, data: Option<&(Vec<String>, Vec<Vec<Value>>)>) -> StepResult {
    let url = cfg_str(step, "url").ok_or_else(|| step_err("This webhook step has no URL"))?.to_owned();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(step_err("Webhook URL must start with http:// or https://"));
    }
    if state.is_offline() && !crate::engine::mcp::is_loopback(&url) {
        return Err(step_err("Offline mode is on. This webhook would leave the device."));
    }
    let workspace = state.workspace().map_err(step_err)?;
    let secret = if step.config.get("secret").and_then(Value::as_bool).unwrap_or(false) {
        let reference = webhook_secret_ref(&workspace.id, &step.id);
        Some(blocking(move || secrets::get(&reference)).await.map_err(|_| step_err("The webhook secret is not in the keychain. Save it again in the editor."))?)
    } else {
        None
    };
    let header = cfg_str(step, "secretHeader").unwrap_or("X-LocalBoard-Secret").to_owned();
    let mut payload = json!({ "automation": name, "summary": summaries.last(), "facts": facts.map(|facts| serde_json::to_value(facts).unwrap_or(Value::Null)) });
    if let Some((columns, rows)) = data {
        payload["columns"] = json!(columns);
        payload["rows"] = json!(rows.iter().take(1_000).collect::<Vec<_>>());
        payload["rowCount"] = json!(rows.len());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(step_err)?;
    let mut request = client.post(&url).json(&payload);
    if let Some(secret) = secret {
        request = request.header(header.as_str(), secret);
    }
    let response = request.send().await.map_err(|error| step_err(format!("Webhook failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(step_err(format!("Webhook answered {status}")));
    }
    let host = url.split('/').nth(2).unwrap_or("").to_owned();
    Ok(plain_out(format!("Posted to {host} ({status})"), json!({ "input": { "url": host, "rows": data.map(|(_, rows)| rows.len()) }, "output": status.as_u16() })))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).and_then(|path| path.canonicalize().ok())
}

fn csv_cell(value: &Value) -> String {
    let text = match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn run_write_file(step: &AutomationStep, name: &str, data: Option<&(Vec<String>, Vec<Vec<Value>>)>) -> StepResult {
    let (columns, rows) = data.ok_or_else(|| step_err("Writing a file needs a SQL step before it"))?;
    let dir = cfg_str(step, "dir").ok_or_else(|| step_err("Choose a folder"))?;
    let dir = PathBuf::from(dir);
    if !dir.is_absolute() || dir.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return Err(step_denied("The folder must be an absolute path without .."));
    }
    let home = home_dir().ok_or_else(|| step_err("Could not find the home folder"))?;
    std::fs::create_dir_all(&dir).map_err(|error| step_err(format!("Could not create the folder: {error}")))?;
    let dir = dir.canonicalize().map_err(step_err)?;
    if !dir.starts_with(&home) {
        return Err(step_denied("Files can only be written under your home folder"));
    }
    let format = if cfg_str(step, "format") == Some("json") { "json" } else { "csv" };
    let overwrite = step.config.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    let base: String = cfg_str(step, "filename")
        .unwrap_or(name)
        .chars()
        .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect();
    let base = if base.trim_matches('_').is_empty() { "automation".to_owned() } else { base };
    let file = if overwrite { format!("{base}.{format}") } else { format!("{base}-{}.{format}", chrono::Local::now().format("%Y%m%d-%H%M%S")) };
    let path = dir.join(file);
    if path.exists() && !overwrite {
        return Err(step_err("That file already exists and overwrite is off"));
    }
    let body = if format == "json" {
        let objects: Vec<Value> = rows
            .iter()
            .map(|row| Value::Object(columns.iter().cloned().zip(row.iter().cloned()).collect()))
            .collect();
        serde_json::to_string_pretty(&objects).map_err(step_err)?
    } else {
        let mut out = columns.iter().map(|column| csv_cell(&Value::String(column.clone()))).collect::<Vec<_>>().join(",");
        for row in rows {
            out.push('\n');
            out.push_str(&row.iter().map(csv_cell).collect::<Vec<_>>().join(","));
        }
        out.push('\n');
        out
    };
    std::fs::write(&path, body).map_err(|error| step_err(format!("Could not write the file: {error}")))?;
    let shown = path.display().to_string();
    Ok(plain_out(format!("Wrote {} rows to {shown}", rows.len()), json!({ "input": { "rows": rows.len(), "format": format }, "output": shown })))
}

/// Continue only when the condition holds; otherwise stop the run without an error.
fn run_branch(step: &AutomationStep, alert: Option<&automations::Alert>, rows: Option<i64>, facts: Option<&Facts>) -> StepResult {
    let _ = facts;
    let kind = cfg_str(step, "if").unwrap_or("alert_fired");
    let op = cfg_str(step, "op").unwrap_or(">");
    let target = step.config.get("value").and_then(|value| value.as_f64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))).unwrap_or(0.0);
    let (met, label) = match kind {
        "alert_fired" => (alert.is_some_and(|alert| alert.fired), "the alert fired".to_owned()),
        "row_count" => {
            let count = rows.unwrap_or(0) as f64;
            let met = match op {
                ">" => count > target,
                ">=" => count >= target,
                "<" => count < target,
                "<=" => count <= target,
                "==" | "=" => (count - target).abs() < f64::EPSILON,
                "!=" => (count - target).abs() >= f64::EPSILON,
                _ => return Err(step_err("Unknown comparison")),
            };
            (met, format!("row count {op} {target}"))
        }
        other => return Err(step_err(format!("Unknown condition: {other}"))),
    };
    let summary = if met { format!("Condition met: {label}") } else { format!("Condition not met ({label}). Stopped here.") };
    let mut out = plain_out(summary, json!({ "input": { "if": kind, "op": op, "value": target, "rows": rows, "fired": alert.map(|alert| alert.fired) }, "output": met }));
    out.stop = !met;
    Ok(out)
}

#[tauri::command]
pub async fn automation_set_step_secret(state: State<'_, AppState>, step_id: String, secret: String) -> AppResult<()> {
    let workspace = state.workspace()?;
    let reference = webhook_secret_ref(&workspace.id, &step_id);
    blocking(move || if secret.is_empty() { secrets::delete(&reference).or(Ok(())) } else { secrets::save(&reference, &secret) }).await
}
