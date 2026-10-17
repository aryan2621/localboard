//! Analysts: who answers, which tables they may use, and the rules, examples, memories and notes they keep.

use std::path::Path;

use tauri::State;

use crate::error::{blocking, AppError, AppResult};
use crate::llm::provider::{LlmProvider, Task};
use crate::llm::router::TaskRouting;
use crate::llm::LlmRuntime;
use crate::retrieval::{self, DOC_PREFIX};
use crate::sql_guard::{self, GuardPolicy};
use crate::state::AppState;
use crate::storage::agents::{
    AgentRecord, EmbedTarget, ExampleRecord, KnowledgeFileRecord, MemoryRecord, RuleRecord, RuleSnapshot, RuleVersionRecord,
};
use crate::storage::conversations::{MessageRole, MessageStatus, QueryStatus};

const MAX_NAME: usize = 80;
const MAX_DESCRIPTION: usize = 2_000;
const MAX_TITLE: usize = 120;
const MAX_RULE: usize = 4_000;
const MAX_QUESTION: usize = 4_000;
const MAX_MEMORY: usize = 1_000;
const EMBED_CHARS: usize = 1_500;

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn required(value: &str, max: usize, label: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid(format!("Enter a {label}")));
    }
    if trimmed.chars().count() > max {
        return Err(AppError::invalid(format!("A {label} is limited to {max} characters")));
    }
    Ok(trimmed.to_owned())
}

fn limited(value: &str, max: usize, label: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.chars().count() > max {
        return Err(AppError::invalid(format!("A {label} is limited to {max} characters")));
    }
    Ok(trimmed.to_owned())
}

fn overrides(routing: TaskRouting) -> AppResult<TaskRouting> {
    let mut clean = TaskRouting::default();
    clean.set(Task::Sql, routing.sql)?;
    clean.set(Task::Reasoning, routing.reasoning)?;
    clean.set(Task::Narrative, routing.narrative)?;
    Ok(clean)
}

pub(crate) async fn document_vector(llm: &LlmRuntime, text: &str) -> Option<Vec<u8>> {
    let provider = match llm.embed_provider().await {
        Ok(Some(provider)) => provider,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!("embedding model did not start: {error}");
            return None;
        }
    };
    match provider.embed(&[format!("{DOC_PREFIX}{}", retrieval::clip(text, EMBED_CHARS))]).await {
        Ok(vectors) => vectors.into_iter().next().map(|vector| retrieval::pack(&vector)),
        Err(error) => {
            tracing::warn!("could not embed saved text: {error}");
            None
        }
    }
}

#[tauri::command]
pub async fn list_agents(state: State<'_, AppState>) -> AppResult<Vec<AgentRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_agents()).await
}

#[tauri::command]
pub async fn create_agent(state: State<'_, AppState>, name: String, description: String) -> AppResult<AgentRecord> {
    state.ensure_unlocked()?;
    let name = required(&name, MAX_NAME, "name")?;
    let description = limited(&description, MAX_DESCRIPTION, "description")?;
    let workspace = state.workspace()?;
    let id = new_id();
    blocking(move || workspace.db()?.insert_agent(&id, &name, &description)).await
}

#[tauri::command]
pub async fn update_agent(
    state: State<'_, AppState>,
    id: String,
    name: String,
    description: String,
    model_overrides: TaskRouting,
    assigned_tables: Vec<String>,
) -> AppResult<AgentRecord> {
    state.ensure_unlocked()?;
    let name = required(&name, MAX_NAME, "name")?;
    let description = limited(&description, MAX_DESCRIPTION, "description")?;
    let model_overrides = overrides(model_overrides)?;
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        let known: Vec<String> = db.list_tables()?.into_iter().map(|table| table.relation).collect();
        let mut tables = Vec::new();
        for requested in assigned_tables {
            let canonical = known
                .iter()
                .find(|relation| relation.eq_ignore_ascii_case(requested.trim()))
                .ok_or_else(|| AppError::invalid(format!("No table named {requested}")))?;
            if !tables.iter().any(|existing: &String| existing == canonical) {
                tables.push(canonical.clone());
            }
        }
        db.update_agent(&id, &name, &description, &model_overrides, &tables)
    })
    .await
}

#[tauri::command]
pub async fn delete_agent(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_agent(&id)).await
}

#[tauri::command]
pub async fn list_rules(state: State<'_, AppState>, agent_id: String) -> AppResult<Vec<RuleRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_rules(&agent_id)).await
}

#[tauri::command]
pub async fn save_rule(
    state: State<'_, AppState>,
    id: Option<String>,
    agent_id: String,
    title: String,
    body: String,
    table_name: Option<String>,
) -> AppResult<RuleRecord> {
    state.ensure_unlocked()?;
    let title = limited(&title, MAX_TITLE, "title")?;
    let body = required(&body, MAX_RULE, "rule")?;
    let table_name = match table_name {
        Some(name) if !name.trim().is_empty() => Some(name.trim().to_owned()),
        _ => None,
    };
    let embedding = document_vector(&state.llm, &body).await;
    let hash = retrieval::content_hash(&body);
    let snapshot = RuleSnapshot { title, body, table_name };
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        let mut snapshot = snapshot;
        if let Some(table) = snapshot.table_name.clone() {
            let known = db.list_tables()?;
            let canonical = known
                .iter()
                .find(|existing| existing.relation.eq_ignore_ascii_case(&table))
                .map(|existing| existing.relation.clone())
                .ok_or_else(|| AppError::invalid(format!("No table named {table}")))?;
            snapshot.table_name = Some(canonical);
        }
        write_rule(&db, id.as_deref(), &agent_id, &snapshot, &hash, embedding.as_deref())
    })
    .await
}

fn write_rule(
    db: &crate::storage::workspace_db::WorkspaceDb,
    id: Option<&str>,
    agent_id: &str,
    snapshot: &RuleSnapshot,
    hash: &str,
    embedding: Option<&[u8]>,
) -> AppResult<RuleRecord> {
    match id {
        Some(id) => db.update_rule(id, &new_id(), snapshot, hash, embedding),
        None => db.insert_rule(&new_id(), &new_id(), agent_id, snapshot, hash, embedding),
    }
}

#[tauri::command]
pub async fn delete_rule(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_rule(&id)).await
}

#[tauri::command]
pub async fn list_rule_versions(state: State<'_, AppState>, rule_id: String) -> AppResult<Vec<RuleVersionRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.rule_versions(&rule_id)).await
}

#[tauri::command]
pub async fn rollback_rule(state: State<'_, AppState>, rule_id: String, version_id: String) -> AppResult<RuleRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let (snapshot, _) = blocking({
        let workspace = workspace.clone();
        let rule_id = rule_id.clone();
        let version_id = version_id.clone();
        move || workspace.db()?.rule_version_snapshot(&rule_id, &version_id)
    })
    .await?;
    let embedding = document_vector(&state.llm, &snapshot.body).await;
    let hash = retrieval::content_hash(&snapshot.body);
    blocking(move || workspace.db()?.restore_rule(&rule_id, &new_id(), &snapshot, &hash, embedding.as_deref())).await
}

#[tauri::command]
pub async fn list_examples(state: State<'_, AppState>, agent_id: String) -> AppResult<Vec<ExampleRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_examples(&agent_id)).await
}

#[tauri::command]
pub async fn save_example(state: State<'_, AppState>, agent_id: String, question: String, sql: String) -> AppResult<ExampleRecord> {
    state.ensure_unlocked()?;
    let question = required(&question, MAX_QUESTION, "question")?;
    let sql = guard_sql(&state, &sql).await?;
    let embedding = document_vector(&state.llm, &question).await;
    let hash = retrieval::content_hash(&question);
    let workspace = state.workspace()?;
    let id = new_id();
    blocking(move || workspace.db()?.insert_example(&id, &agent_id, &question, &sql, "manual", None, &hash, embedding.as_deref())).await
}

#[tauri::command]
pub async fn delete_example(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_example(&id)).await
}

/// Saves a thumbs-up answer as a reviewed example for the analyst that wrote it.
#[tauri::command]
pub async fn save_answer_example(state: State<'_, AppState>, message_id: String) -> AppResult<ExampleRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let prepared = blocking({
        let workspace = workspace.clone();
        let message_id = message_id.clone();
        move || {
            let db = workspace.db()?;
            if db.example_for_message(&message_id)?.is_some() {
                return Err(AppError::invalid("This answer is already saved as an example"));
            }
            let message = db.message(&message_id)?;
            if message.role != MessageRole::Assistant || message.status != MessageStatus::Done {
                return Err(AppError::invalid("Only a finished answer can be saved as an example"));
            }
            let answer = message.answer.ok_or_else(|| AppError::invalid("This answer has no query to save"))?;
            let query_id = answer
                .get("primaryQueryId")
                .and_then(|value| value.as_str())
                .ok_or_else(|| AppError::invalid("This answer has no query to save"))?;
            let query = db.query(query_id)?;
            if query.status != QueryStatus::Ok {
                return Err(AppError::invalid("Only a query that ran successfully can be saved"));
            }
            let question = db
                .messages(&message.conversation_id)?
                .into_iter()
                .find(|earlier| earlier.seq + 1 == message.seq && earlier.role == MessageRole::User)
                .map(|earlier| earlier.content)
                .ok_or_else(|| AppError::invalid("The question for this answer is missing"))?;
            let agent_id = answer
                .get("agentId")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .or_else(|| db.list_agents().ok().and_then(|agents| agents.into_iter().next().map(|agent| agent.id)))
                .ok_or_else(|| AppError::invalid("Create an analyst before saving an example"))?;
            Ok((question, query.sql, agent_id))
        }
    })
    .await?;
    let (question, sql, agent_id) = prepared;
    let question = required(&question, MAX_QUESTION, "question")?;
    let sql = guard_sql(&state, &sql).await?;
    let embedding = document_vector(&state.llm, &question).await;
    let hash = retrieval::content_hash(&question);
    let id = new_id();
    let message_id_for_row = message_id.clone();
    blocking(move || {
        let db = workspace.db()?;
        let saved = db.insert_example(&id, &agent_id, &question, &sql, "thumbs_up", Some(&message_id_for_row), &hash, embedding.as_deref())?;
        db.set_feedback(&message_id, Some(1))?;
        Ok(saved)
    })
    .await
}

#[tauri::command]
pub async fn list_memories(state: State<'_, AppState>, agent_id: String) -> AppResult<Vec<MemoryRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_memories(&agent_id)).await
}

#[tauri::command]
pub async fn save_memory(state: State<'_, AppState>, id: Option<String>, agent_id: String, body: String) -> AppResult<MemoryRecord> {
    state.ensure_unlocked()?;
    let body = required(&body, MAX_MEMORY, "memory")?;
    let hash = retrieval::memory_key(&body);
    let embedding = document_vector(&state.llm, &body).await;
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        match id {
            Some(id) => db.update_memory(&id, &body, &hash, embedding.as_deref()),
            None => db.insert_memory(&new_id(), &agent_id, &body, "active", None, &hash, embedding.as_deref()),
        }
    })
    .await
}

#[tauri::command]
pub async fn set_memory_status(state: State<'_, AppState>, id: String, status: String) -> AppResult<MemoryRecord> {
    state.ensure_unlocked()?;
    if !matches!(status.as_str(), "proposed" | "active" | "archived") {
        return Err(AppError::invalid("Status must be proposed, active or archived"));
    }
    let workspace = state.workspace()?;
    let memory = blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        let status = status.clone();
        move || workspace.db()?.set_memory_status(&id, &status)
    })
    .await?;
    if status == "active" && memory.embedding.is_none() {
        if let Some(blob) = document_vector(&state.llm, &memory.body).await {
            let workspace = state.workspace()?;
            blocking(move || workspace.db()?.store_embedding(EmbedTarget::Memory, &id, &blob)).await?;
        }
    }
    Ok(memory)
}

#[tauri::command]
pub async fn delete_memory(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_memory(&id)).await
}

#[tauri::command]
pub async fn list_knowledge(state: State<'_, AppState>, agent_id: String) -> AppResult<Vec<KnowledgeFileRecord>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_knowledge(&agent_id)).await
}

#[tauri::command]
pub async fn add_knowledge(state: State<'_, AppState>, agent_id: String, path: String) -> AppResult<KnowledgeFileRecord> {
    state.ensure_unlocked()?;
    let path = Path::new(&path).to_path_buf();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| AppError::invalid("That file has no name"))?
        .chars()
        .take(200)
        .collect::<String>();
    let chunks = blocking(move || retrieval::read_knowledge(&path)).await?;
    let rows: Vec<(String, String, String)> =
        chunks.into_iter().map(|body| (new_id(), body.clone(), retrieval::content_hash(&body))).collect();
    let file_id = new_id();
    let workspace = state.workspace()?;
    let record = blocking({
        let workspace = workspace.clone();
        let rows = rows.clone();
        let agent_id = agent_id.clone();
        let file_id = file_id.clone();
        let name = name.clone();
        move || {
            let db = workspace.db()?;
            db.insert_knowledge(&file_id, &agent_id, &name, &rows)?;
            db.list_knowledge(&agent_id)?
                .into_iter()
                .find(|file| file.id == file_id)
                .ok_or_else(|| AppError::Internal("the document was saved but could not be read back".into()))
        }
    })
    .await?;
    match state.llm.embed_provider().await {
        Ok(Some(provider)) => {
            let inputs: Vec<String> = rows.iter().map(|(_, body, _)| format!("{DOC_PREFIX}{}", retrieval::clip(body, EMBED_CHARS))).collect();
            if let Ok(vectors) = provider.embed(&inputs).await {
                if vectors.len() == rows.len() {
                    let blobs: Vec<(String, Vec<u8>)> =
                        rows.iter().zip(vectors).map(|((id, _, _), vector)| (id.clone(), retrieval::pack(&vector))).collect();
                    let workspace = state.workspace()?;
                    if let Err(error) = blocking(move || {
                        let db = workspace.db()?;
                        for (id, blob) in &blobs {
                            db.store_embedding(EmbedTarget::Chunk, id, blob)?;
                        }
                        Ok(())
                    })
                    .await
                    {
                        tracing::warn!("saved the document but could not store its embeddings: {error}");
                    }
                }
            }
        }
        Ok(None) => {}
        Err(error) => tracing::warn!("saved the document but the embedding model did not start: {error}"),
    }
    Ok(record)
}

#[tauri::command]
pub async fn delete_knowledge(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_knowledge(&id)).await
}

async fn guard_sql(state: &AppState, sql: &str) -> AppResult<String> {
    let workspace = state.workspace()?;
    let sql = sql.to_owned();
    blocking(move || {
        let db = workspace.db()?;
        let policy = GuardPolicy::new(db.list_tables()?.into_iter().map(|table| table.relation));
        let validated = sql_guard::validate(&sql, &policy).map_err(|error| AppError::invalid(error.to_string()))?;
        Ok(validated.sql)
    })
    .await
}
