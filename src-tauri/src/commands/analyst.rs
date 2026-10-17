use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use super::ANALYST_EVENT;
use crate::engine::analyst::{context, export, AnalystEvent, EventSink, Models, Run};
use crate::engine::query::{self, elapsed_ms, CancelToken};
use crate::error::{blocking, AppError, AppResult};
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::sql_guard::{self, GuardPolicy};
use crate::state::AppState;
use crate::storage::conversations::{
    snapshot, title_from, ConversationSummary, MessageRecord, MessageRole, MessageStatus, QueryRecord, QueryStatus,
};
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};

/// Matches the SQL guard's text limit, since SQL-only mode sends the question as the query.
const MAX_QUESTION_CHARS: usize = 20_000;
const MAX_SEARCH_CHARS: usize = 200;
const COMPLETION_QUESTIONS: usize = 200;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskStarted {
    pub conversation_id: String,
    pub user_message_id: String,
    pub assistant_message_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalystEventPayload<'a> {
    conversation_id: &'a str,
    message_id: &'a str,
    event: &'a AnalystEvent,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub id: String,
    pub title: String,
    pub messages: Vec<MessageRecord>,
    pub queries: Vec<QueryRecord>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Starts an answer in the background; progress arrives as [`ANALYST_EVENT`] events.
#[tauri::command]
pub async fn ask(
    app: AppHandle,
    state: State<'_, AppState>,
    question: String,
    conversation_id: Option<String>,
) -> AppResult<AskStarted> {
    let question = question.trim().to_owned();
    if question.is_empty() {
        return Err(AppError::invalid("Type a question first"));
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(AppError::invalid(format!("Questions are limited to {MAX_QUESTION_CHARS} characters")));
    }
    let workspace = state.workspace()?;
    let routing: TaskRouting = state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default();
    let models = Models::available(state.llm.clone(), routing);
    let offline = state.is_offline();

    let (user_message_id, assistant_message_id) = (new_id(), new_id());
    let cancel = state.analyst_runs.begin(&assistant_message_id)?;
    let stored = {
        let (workspace, question, conversation_id) = (workspace.clone(), question.clone(), conversation_id.clone());
        let (user_id, assistant_id) = (user_message_id.clone(), assistant_message_id.clone());
        blocking(move || {
            let db = workspace.db()?;
            let conversation_id = match conversation_id {
                Some(id) => {
                    db.conversation_title(&id)?;
                    id
                }
                None => {
                    let id = new_id();
                    db.create_conversation(&id, &title_from(&question))?;
                    id
                }
            };
            db.insert_message(&user_id, &conversation_id, MessageRole::User, &question, MessageStatus::Done)?;
            db.insert_message(&assistant_id, &conversation_id, MessageRole::Assistant, "", MessageStatus::Pending)?;
            Ok(conversation_id)
        })
        .await
    };
    let conversation_id = match stored {
        Ok(id) => id,
        Err(error) => {
            state.analyst_runs.finish(&assistant_message_id);
            return Err(error);
        }
    };

    let sink: EventSink = {
        let (app, conversation_id, message_id) = (app.clone(), conversation_id.clone(), assistant_message_id.clone());
        Arc::new(move |event: AnalystEvent| {
            let payload = AnalystEventPayload { conversation_id: &conversation_id, message_id: &message_id, event: &event };
            if let Err(error) = app.emit(ANALYST_EVENT, payload) {
                tracing::warn!("could not deliver an analyst event: {error}");
            }
        })
    };
    let mut run = Run::new(workspace, models, cancel, sink, conversation_id.clone(), assistant_message_id.clone(), question);
    run.set_offline(offline);
    let message_id = assistant_message_id.clone();
    tauri::async_runtime::spawn(async move {
        run.execute().await;
        app.state::<AppState>().analyst_runs.finish(&message_id);
    });

    Ok(AskStarted { conversation_id, user_message_id, assistant_message_id })
}

#[tauri::command]
pub fn cancel_answer(state: State<'_, AppState>, message_id: String) -> AppResult<()> {
    state.analyst_runs.cancel(&message_id)
}

#[tauri::command]
pub async fn list_conversations(state: State<'_, AppState>, search: Option<String>) -> AppResult<Vec<ConversationSummary>> {
    let search = search.map(|text| text.trim().chars().take(MAX_SEARCH_CHARS).collect::<String>()).filter(|text| !text.is_empty());
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_conversations(search.as_deref())).await
}

#[tauri::command]
pub async fn get_conversation(state: State<'_, AppState>, id: String) -> AppResult<ConversationDetail> {
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        Ok(ConversationDetail { title: db.conversation_title(&id)?, messages: db.messages(&id)?, queries: db.conversation_queries(&id)?, id })
    })
    .await
}

#[tauri::command]
pub async fn rename_conversation(state: State<'_, AppState>, id: String, title: String) -> AppResult<()> {
    let title = title.trim().chars().take(200).collect::<String>();
    if title.is_empty() {
        return Err(AppError::invalid("Give the chat a name"));
    }
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.rename_conversation(&id, &title)).await
}

#[tauri::command]
pub async fn delete_conversation(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        if db.messages(&id)?.iter().any(|message| message.status == MessageStatus::Pending) {
            return Err(AppError::Conflict("Stop the answer being written before deleting this chat.".into()));
        }
        db.delete_conversation(&id)
    })
    .await
}

#[tauri::command]
pub async fn export_conversation(state: State<'_, AppState>, id: String, path: String) -> AppResult<()> {
    let path = PathBuf::from(path);
    if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("md")) {
        return Err(AppError::invalid("Choose a file name ending in .md"));
    }
    let workspace = state.workspace()?;
    blocking(move || {
        let markdown = {
            let db = workspace.db()?;
            export::markdown(&db.conversation_title(&id)?, &db.messages(&id)?, &db.conversation_queries(&id)?)
        };
        std::fs::write(&path, markdown)?;
        Ok(())
    })
    .await
}

/// Runs a stored query again against the current data and keeps the fresh result.
#[tauri::command]
pub async fn rerun_query(state: State<'_, AppState>, id: String) -> AppResult<QueryRecord> {
    let workspace = state.workspace()?;
    blocking(move || {
        let (mut record, policy) = {
            let db = workspace.db()?;
            (db.query(&id)?, GuardPolicy::new(db.list_tables()?.into_iter().map(|table| table.relation)))
        };
        let started = Instant::now();
        let outcome = match sql_guard::validate(&record.sql, &policy) {
            Err(violation) => Err((QueryStatus::Denied, violation.to_string())),
            Ok(validated) => {
                record.sql = validated.sql.clone();
                record.relations = validated.relations.clone();
                query::run(&workspace.query_connection()?, &validated, &std::collections::HashMap::new(), &CancelToken::default())
                    .map_err(|error| (QueryStatus::Error, error.to_string()))
            }
        };
        record.duration_ms = Some(elapsed_ms(started));
        match outcome {
            Ok(result) => {
                record.status = QueryStatus::Ok;
                record.row_count = Some(result.rows.len() as i64);
                record.result = Some(snapshot(&result));
                record.error = None;
            }
            Err((status, message)) => {
                record.status = status;
                record.row_count = None;
                record.result = None;
                record.error = Some(message);
            }
        }
        let db = workspace.db()?;
        db.update_query_run(&record)?;
        db.record_audit_lineage(
            &AuditEntry {
                kind: "analyst.rerun",
                status: match record.status {
                    QueryStatus::Ok => AuditStatus::Ok,
                    QueryStatus::Error => AuditStatus::Error,
                    QueryStatus::Denied => AuditStatus::Denied,
                },
                duration_ms: record.duration_ms,
                query: AuditQuery { question: None, sql: Some(&record.sql), rows: record.row_count },
                detail: json!({ "query": record.id, "error": record.error }),
            },
            &record.relations,
        )?;
        if !record.relations.is_empty() {
            db.replace_query_relations("query", &record.id, &record.relations)?;
        }
        Ok(record)
    })
    .await
}

#[tauri::command]
pub async fn set_feedback(state: State<'_, AppState>, message_id: String, feedback: Option<i8>) -> AppResult<()> {
    if feedback.is_some_and(|value| value != 1 && value != -1) {
        return Err(AppError::invalid("Feedback must be 1, -1 or empty"));
    }
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.set_feedback(&message_id, feedback)).await
}

#[tauri::command]
pub async fn starter_questions(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let workspace = state.workspace()?;
    blocking(move || context::starter_questions(&workspace)).await
}

#[derive(Serialize)]
pub struct CompletionSources {
    questions: Vec<String>,
    terms: Vec<String>,
}

/// What the prompt box completes from: earlier questions, then starter questions, plus schema terms.
#[tauri::command]
pub async fn completion_sources(state: State<'_, AppState>) -> AppResult<CompletionSources> {
    let workspace = state.workspace()?;
    blocking(move || {
        let mut questions = workspace.db()?.recent_questions(COMPLETION_QUESTIONS)?;
        for starter in context::starter_questions(&workspace)? {
            if !questions.contains(&starter) {
                questions.push(starter);
            }
        }
        Ok(CompletionSources { questions, terms: context::vocabulary(&workspace)? })
    })
    .await
}
