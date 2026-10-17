//! The AI analyst: question → relevant schema → constrained SQL → guard → execution with repair
//! → sanity checks → code-computed facts → narrated answer. Every step is streamed to the UI.

pub mod chart;
pub mod context;
pub mod export;
pub mod facts;
mod forecast;
mod knowledge;
mod numcheck;
pub mod prompts;
mod why;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::engine::query::{self, elapsed_ms, CancelToken, QueryError, QueryResult};
use crate::error::{blocking, AppError};
use crate::llm::openai_compat::OpenAiCompatProvider;
use crate::llm::provider::{ChatMessage, ChatRequest, LlmError, LlmProvider, ResponseFormat, Role, Task};
use crate::llm::router::TaskRouting;
use crate::llm::LlmRuntime;
use crate::sql_guard::{self, GuardPolicy};
use crate::state::Workspace;
use crate::storage::agents::RuleRecord;
use crate::storage::conversations::{snapshot, MessageRole, MessageStatus, QueryPurpose, QueryRecord, QueryStatus};
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus};
use chart::{ChartHint, ChartSpec};
use context::SchemaContext;
use facts::Anomaly;
use prompts::{Generated, Intent};

const MAX_REPAIRS: usize = 3;
const HISTORY_TURNS: usize = 3;
const HISTORY_ANSWER_CHARS: usize = 300;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerMode {
    #[default]
    Ai,
    Sql,
    Why,
    Clarification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Running,
    Done,
    Error,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub label: String,
    pub status: StepStatus,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clarification {
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Driver {
    pub dimension: String,
    pub segment: Value,
    pub previous: f64,
    pub current: f64,
    pub change: f64,
    pub share_of_change: Option<f64>,
}

/// One sub-question of a multi-part answer and the chart of its query.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnswerPart {
    pub question: String,
    pub chart: ChartSpec,
}

/// A standing instruction the analyst noticed and is asking to keep.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProposal {
    pub id: String,
    pub body: String,
}

/// Everything about an answer except its text and queries, stored with the message.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub mode: AnswerMode,
    pub steps: Vec<Step>,
    pub explanation: Option<String>,
    pub assumptions: Vec<String>,
    pub warnings: Vec<String>,
    pub clarification: Option<Clarification>,
    pub chart: Option<ChartSpec>,
    pub primary_query_id: Option<String>,
    pub drivers: Vec<Driver>,
    pub model: Option<String>,
    #[serde(default)]
    pub anomalies: Vec<Anomaly>,
    #[serde(default)]
    pub parts: Vec<AnswerPart>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub memory_proposal: Option<MemoryProposal>,
    /// What was in the model's context for this answer. Absent on answers saved before it was recorded.
    #[serde(default)]
    pub lineage: Option<Lineage>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lineage {
    pub model_id: Option<String>,
    pub rules: Vec<LineageRule>,
    pub examples: Vec<String>,
    pub memories: Vec<String>,
    pub knowledge: Vec<LineageChunk>,
    pub columns: Vec<String>,
}

/// A rule as the analyst saw it. The version is the rule's last-updated stamp.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageRule {
    pub id: String,
    pub title: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageChunk {
    pub doc: String,
    pub chunk: i64,
}

/// What answering one query produced: its chart and the code-computed facts for narration.
struct PartOutcome {
    query_id: String,
    chart: ChartSpec,
    facts: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AnalystEvent {
    Step { step: Step },
    Token { text: String },
    Done { status: MessageStatus },
}

pub type EventSink = Arc<dyn Fn(AnalystEvent) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
enum AnalystError {
    #[error("Stopped.")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

impl From<AppError> for AnalystError {
    fn from(error: AppError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<LlmError> for AnalystError {
    fn from(error: LlmError) -> Self {
        Self::Failed(error.to_string())
    }
}

/// The models available for analysis. Absent means SQL-only mode.
pub struct Models {
    llm: Arc<LlmRuntime>,
    routing: TaskRouting,
}

impl Models {
    pub fn available(llm: Arc<LlmRuntime>, routing: TaskRouting) -> Option<Self> {
        (llm.runtime_available() && llm.resolve(&routing, Task::Sql).is_some()).then_some(Self { llm, routing })
    }

    async fn provider(&self, task: Task, routing: &TaskRouting) -> Result<OpenAiCompatProvider, LlmError> {
        let spec = self.llm.resolve(routing, task).ok_or(LlmError::NoModel)?;
        self.llm.provider(spec.id).await
    }
}

/// One answer being written. Created by the command layer, consumed by [`Run::execute`].
pub struct Run {
    workspace: Arc<Workspace>,
    models: Option<Models>,
    cancel: Arc<CancelToken>,
    sink: EventSink,
    conversation_id: String,
    message_id: String,
    question: String,
    answer: Answer,
    content: String,
    attempts: i64,
    policy: GuardPolicy,
    largest_source_rows: HashMap<String, i64>,
    /// Effective per-task models: the workspace routing, with this analyst's overrides applied.
    routing: TaskRouting,
    assigned_tables: Vec<String>,
    guidance: String,
    query_embedding: Option<Vec<f32>>,
    pinned_agent: Option<String>,
    rule_override: Option<Vec<RuleRecord>>,
    score_only: bool,
    captured: Option<(String, QueryResult)>,
    retrieved_docs: bool,
    offline: bool,
}

fn looks_like_sql(text: &str) -> bool {
    let lowered = text.trim_start().to_lowercase();
    lowered.starts_with("select ") || lowered.starts_with("with ") || lowered.starts_with("select\n")
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max { text.to_owned() } else { format!("{}…", text.chars().take(max).collect::<String>()) }
}

impl Run {
    pub fn new(
        workspace: Arc<Workspace>,
        models: Option<Models>,
        cancel: Arc<CancelToken>,
        sink: EventSink,
        conversation_id: String,
        message_id: String,
        question: String,
    ) -> Self {
        Self {
            workspace,
            models,
            cancel,
            sink,
            conversation_id,
            message_id,
            question,
            answer: Answer::default(),
            content: String::new(),
            attempts: 0,
            policy: GuardPolicy::new(Vec::new()),
            largest_source_rows: HashMap::new(),
            routing: TaskRouting::default(),
            assigned_tables: Vec::new(),
            guidance: String::new(),
            query_embedding: None,
            pinned_agent: None,
            rule_override: None,
            score_only: false,
            captured: None,
            retrieved_docs: false,
            offline: false,
        }
    }

    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    async fn call_allowed_tools(&mut self) -> Result<(), AnalystError> {
        let Some(agent_id) = self.answer.agent_id.clone() else { return Ok(()) };
        let workspace = self.workspace.clone();
        let servers = blocking({
            let workspace = workspace.clone();
            move || Ok(workspace.db()?.list_mcp_servers()?.into_iter().filter(|server| server.enabled).collect::<Vec<_>>())
        })
        .await?;
        if servers.is_empty() {
            return Ok(());
        }
        if self.retrieved_docs {
            self.answer.warnings.push(
                "Tools were not called because this question used retrieved documents. Confirm a tool from Agents if you need it.".into(),
            );
            return Ok(());
        }
        self.step("tools", "Calling allowed tools");
        let mut used = 0;
        let question = self.question.to_ascii_lowercase();
        for server in servers {
            if used >= 2 {
                break;
            }
            let tools = match crate::engine::mcp::list_remote_tools(&server, self.offline).await {
                Ok(tools) => tools,
                Err(error) => {
                    self.answer.warnings.push(format!("{}: {error}", server.name));
                    continue;
                }
            };
            for tool in tools {
                if used >= 2 {
                    break;
                }
                let allowed = workspace
                    .db()
                    .ok()
                    .and_then(|db| db.mcp_rule(&agent_id, &server.id, &tool.name).ok().flatten())
                    .map(|rule| rule.allow)
                    .unwrap_or(!crate::engine::mcp::is_write_like(&tool.name));
                if !allowed || !tool.read_only {
                    continue;
                }
                if !question.contains(&tool.name.to_ascii_lowercase()) {
                    continue;
                }
                match crate::engine::mcp::call_remote(
                    &server,
                    "tools/call",
                    serde_json::json!({ "name": tool.name, "arguments": { "question": self.question } }),
                    self.offline,
                )
                .await
                {
                    Ok(result) => {
                        crate::engine::mcp::audit_tool(&workspace, &server.name, &tool.name, true, crate::storage::workspace_db::AuditStatus::Ok);
                        let text = crate::engine::mcp::wrap_untrusted(&result.to_string());
                        self.guidance.push_str(&format!("\nTOOL {} / {}:\n{text}\n", server.name, tool.name));
                        used += 1;
                    }
                    Err(error) => {
                        crate::engine::mcp::audit_tool(&workspace, &server.name, &tool.name, true, crate::storage::workspace_db::AuditStatus::Error);
                        self.answer.warnings.push(format!("{} / {}: {error}", server.name, tool.name));
                    }
                }
            }
        }
        self.finish_step("tools", StepStatus::Done, Some(if used == 0 { "none matched".into() } else { format!("{used} call(s)") }));
        Ok(())
    }

    /// Runs the pipeline to completion, cancellation or failure, then stores the outcome.
    pub async fn execute(mut self) -> String {
        let cancel = self.cancel.clone();
        let outcome = tokio::select! {
            outcome = self.pipeline() => outcome,
            () = cancel.cancelled() => Err(AnalystError::Cancelled),
        };
        let status = match outcome {
            Ok(()) => MessageStatus::Done,
            Err(AnalystError::Cancelled) => {
                self.close_running_steps(StepStatus::Skipped);
                if self.content.is_empty() {
                    self.content = "Stopped before an answer was ready.".to_owned();
                }
                MessageStatus::Cancelled
            }
            Err(AnalystError::Failed(message)) => {
                self.close_running_steps(StepStatus::Error);
                self.content = message;
                MessageStatus::Error
            }
        };
        let (workspace, id, content, answer) = (self.workspace.clone(), self.message_id.clone(), self.content.clone(), self.answer.clone());
        if let Err(error) = blocking(move || workspace.db()?.finish_message(&id, &content, status, &answer)).await {
            tracing::error!("could not save answer {}: {error}", self.message_id);
        }
        (self.sink)(AnalystEvent::Done { status });
        self.content
    }

    // Steps ---------------------------------------------------------------------------------------

    fn emit_step(&self, index: usize) {
        (self.sink)(AnalystEvent::Step { step: self.answer.steps[index].clone() });
    }

    fn step(&mut self, id: &str, label: impl Into<String>) {
        let step = Step { id: id.to_owned(), label: label.into(), status: StepStatus::Running, detail: None };
        let index = match self.answer.steps.iter().position(|existing| existing.id == id) {
            Some(index) => {
                self.answer.steps[index] = step;
                index
            }
            None => {
                self.answer.steps.push(step);
                self.answer.steps.len() - 1
            }
        };
        self.emit_step(index);
    }

    fn finish_step(&mut self, id: &str, status: StepStatus, detail: Option<String>) {
        if let Some(index) = self.answer.steps.iter().position(|step| step.id == id) {
            self.answer.steps[index].status = status;
            self.answer.steps[index].detail = detail;
            self.emit_step(index);
        }
    }

    fn close_running_steps(&mut self, status: StepStatus) {
        let running: Vec<String> =
            self.answer.steps.iter().filter(|step| step.status == StepStatus::Running).map(|step| step.id.clone()).collect();
        for id in running {
            self.finish_step(&id, status, None);
        }
    }

    // Pipeline ------------------------------------------------------------------------------------

    async fn pipeline(&mut self) -> Result<(), AnalystError> {
        let workspace = self.workspace.clone();
        let tables = blocking(move || workspace.db()?.list_tables()).await?;
        if tables.is_empty() {
            return Err(AnalystError::Failed("Add a file or database in Connections first, then ask again.".into()));
        }
        self.largest_source_rows =
            tables.iter().filter_map(|table| Some((table.relation.clone(), table.row_count?))).collect();
        self.policy = GuardPolicy::new(tables.into_iter().map(|table| table.relation));

        if self.models.is_none() || looks_like_sql(&self.question) {
            return self.direct_sql().await;
        }
        self.routing = self.models.as_ref().map(|models| models.routing.clone()).unwrap_or_default();
        self.choose_agent().await?;

        self.step("context", "Finding the relevant tables");
        let (workspace, question, only) = (self.workspace.clone(), self.question.clone(), self.assigned_tables.clone());
        let context = blocking(move || context::build(&workspace, &question, &only)).await?;
        let names: Vec<&str> = context.tables.iter().map(|table| table.relation.as_str()).collect();
        self.finish_step("context", StepStatus::Done, Some(names.join(", ")));
        let table_names: Vec<String> = context.tables.iter().map(|table| table.relation.clone()).collect();
        self.recall(&table_names).await?;
        if !self.score_only {
            self.propose_memory().await?;
            self.call_allowed_tools().await?;
        }

        self.load_model(Task::Sql).await?;
        self.step("sql", "Writing SQL");
        let history = self.history().await?;
        let mut messages = history.clone();
        messages.push(ChatMessage { role: Role::User, content: self.question.clone() });
        let generated: Generated = self
            .ask_json(Task::Sql, prompts::generation_system(&context.prompt, &self.guidance), messages, "analysis", prompts::generation_schema(true))
            .await?;
        self.finish_step("sql", StepStatus::Done, Some(generated.explanation.clone()));

        if generated.intent == Intent::Ambiguous && !generated.clarification_question.trim().is_empty() {
            self.answer.mode = AnswerMode::Clarification;
            self.content = generated.clarification_question.trim().to_owned();
            self.answer.clarification =
                Some(Clarification { question: self.content.clone(), options: generated.clarification_options });
            return Ok(());
        }
        if generated.intent == Intent::Why && self.explain_change(&context).await? {
            return Ok(());
        }
        let plan: Vec<String> =
            generated.plan.iter().map(|part| part.trim().to_owned()).filter(|part| !part.is_empty()).take(prompts::MAX_PLAN_PARTS).collect();
        if generated.sql.trim().is_empty() && plan.len() >= 2 {
            return self.answer_in_parts(&context, history, plan).await;
        }

        self.answer.explanation = Some(generated.explanation.clone());
        self.answer.assumptions = generated.assumptions.clone();
        let part = self.answer_part(&context, &generated, "").await?;
        self.answer.chart = Some(part.chart);
        self.answer.primary_query_id = Some(part.query_id);
        let mut facts = part.facts;
        facts["assumptions"] = json!(self.answer.assumptions);
        facts["warnings"] = json!(self.answer.warnings);
        self.narrate(facts).await
    }

    /// Runs one generated query through repair, the sanity check and (for forecasts) projection.
    /// `suffix` keeps step ids unique when several parts run in one answer.
    async fn answer_part(&mut self, context: &SchemaContext, generated: &Generated, suffix: &str) -> Result<PartOutcome, AnalystError> {
        if generated.sql.trim().is_empty() {
            return Err(AnalystError::Failed("The AI model did not write a query for this question. Try rephrasing it.".into()));
        }
        let (record, result) = self.answer_query(context, generated.sql.clone(), suffix).await?;
        let (mut record, mut result) = self.sanity_check(context, record, result, suffix).await?;
        let mut hint = generated.chart_hint;
        let mut projection = None;
        if generated.intent == Intent::Forecast {
            if let Some((projected, projected_result)) =
                self.project(&record, &result, generated.forecast_periods, &format!("forecast{suffix}")).await?
            {
                projection = Some(forecast::summary(&projected_result));
                (record, result) = (projected, projected_result);
                hint = ChartHint::Line;
            }
        }
        let mut computed = facts::compute(&result);
        context::fence_preview(&mut computed.preview);
        self.answer.anomalies.extend(computed.anomalies.iter().cloned());
        let mut facts = serde_json::to_value(&computed).map_err(AppError::from)?;
        let mut chart = chart::pick(&record.id, &result, hint);
        if let Some(projection) = projection {
            facts["forecast"] = projection;
            chart.dashed = vec![forecast::FORECAST_COLUMN.to_owned()];
        }
        Ok(PartOutcome { query_id: record.id, chart, facts })
    }

    /// Answers each sub-question of the plan with its own query. A part that fails becomes a
    /// warning; the answer fails only if every part does.
    async fn answer_in_parts(&mut self, context: &SchemaContext, history: Vec<ChatMessage>, plan: Vec<String>) -> Result<(), AnalystError> {
        self.step("plan", format!("Splitting the question into {} parts", plan.len()));
        self.finish_step("plan", StepStatus::Done, Some(plan.join(" · ")));
        let mut parts = Vec::new();
        for (index, question) in plan.into_iter().enumerate() {
            let suffix = format!("-{}", index + 1);
            let sql_step = format!("sql{suffix}");
            self.step(&sql_step, format!("Writing SQL for part {}: {question}", index + 1));
            let mut messages = history.clone();
            messages.push(ChatMessage { role: Role::User, content: prompts::part_user(&self.question, &question) });
            let generated: Generated = self
                .ask_json(Task::Sql, prompts::generation_system(&context.prompt, &self.guidance), messages, "analysis", prompts::generation_schema(false))
                .await?;
            self.finish_step(&sql_step, StepStatus::Done, Some(generated.explanation.clone()));
            self.answer.assumptions.extend(generated.assumptions.iter().cloned());
            match self.answer_part(context, &generated, &suffix).await {
                Ok(outcome) => parts.push((question, outcome)),
                Err(AnalystError::Failed(message)) => self.answer.warnings.push(format!("Part {} ({question}): {message}", index + 1)),
                Err(cancelled) => return Err(cancelled),
            }
        }
        let Some((_, first)) = parts.first() else {
            return Err(AnalystError::Failed(format!("None of the parts could be answered. {}", self.answer.warnings.join(" "))));
        };
        self.answer.primary_query_id = Some(first.query_id.clone());
        self.answer.explanation = Some(format!("Answered in {} parts, one query each.", parts.len()));
        self.answer.parts =
            parts.iter().map(|(question, outcome)| AnswerPart { question: question.clone(), chart: outcome.chart.clone() }).collect();
        let facts = json!({
            "parts": parts.iter().map(|(question, outcome)| json!({ "question": question, "facts": outcome.facts })).collect::<Vec<_>>(),
            "assumptions": self.answer.assumptions,
            "warnings": self.answer.warnings,
        });
        self.narrate(facts).await
    }

    async fn direct_sql(&mut self) -> Result<(), AnalystError> {
        self.answer.mode = AnswerMode::Sql;
        self.step("run", "Running your SQL");
        let sql = self.question.clone();
        match self.execute_once(QueryPurpose::Answer, &sql).await? {
            Ok((record, result)) => {
                self.finish_step("run", StepStatus::Done, None);
                let rows = result.rows.len();
                self.content = if result.truncated {
                    format!("Showing the first {rows} rows.")
                } else {
                    format!("{rows} {}.", if rows == 1 { "row" } else { "rows" })
                };
                self.answer.warnings = facts::sanity_warnings(&record.sql, &result, None);
                self.answer.chart = Some(chart::pick(&record.id, &result, ChartHint::Auto));
                self.answer.primary_query_id = Some(record.id);
                Ok(())
            }
            Err(error) => Err(AnalystError::Failed(error)),
        }
    }

    async fn load_model(&mut self, task: Task) -> Result<(), AnalystError> {
        let models = self.models.as_ref().ok_or(LlmError::NoModel)?;
        let spec = models.llm.resolve(&self.routing, task).ok_or(LlmError::NoModel)?;
        let running = models.llm.running_model().await;
        self.answer.model = Some(spec.name.to_owned());
        self.answer.lineage.get_or_insert_with(Lineage::default).model_id = Some(spec.id.to_owned());
        if running.as_deref() == Some(spec.id) {
            return Ok(());
        }
        self.step("model", format!("Starting {}", spec.name));
        self.models.as_ref().ok_or(LlmError::NoModel)?.provider(task, &self.routing).await?;
        self.finish_step("model", StepStatus::Done, None);
        Ok(())
    }

    /// Earlier turns of this conversation, so follow-up questions keep their context.
    async fn history(&self) -> Result<Vec<ChatMessage>, AnalystError> {
        if self.score_only {
            return Ok(Vec::new());
        }
        let (workspace, conversation_id, current) = (self.workspace.clone(), self.conversation_id.clone(), self.message_id.clone());
        let (messages, queries) = blocking(move || {
            let db = workspace.db()?;
            Ok((db.messages(&conversation_id)?, db.conversation_queries(&conversation_id)?))
        })
        .await?;
        let sql_by_id: HashMap<&str, &str> = queries.iter().map(|query| (query.id.as_str(), query.sql.as_str())).collect();
        let finished: Vec<_> = messages.iter().filter(|message| message.id != current && message.status == MessageStatus::Done).collect();
        let mut turns = Vec::new();
        for pair in finished.windows(2) {
            let (asked, answered) = (pair[0], pair[1]);
            if asked.role != MessageRole::User || answered.role != MessageRole::Assistant {
                continue;
            }
            let sql = answered
                .answer
                .as_ref()
                .and_then(|answer| answer.get("primaryQueryId"))
                .and_then(Value::as_str)
                .and_then(|id| sql_by_id.get(id));
            let reply = match sql {
                Some(sql) => format!("SQL:\n{sql}\nAnswer: {}", clip(&answered.content, HISTORY_ANSWER_CHARS)),
                None => clip(&answered.content, HISTORY_ANSWER_CHARS),
            };
            turns.push([
                ChatMessage { role: Role::User, content: asked.content.clone() },
                ChatMessage { role: Role::Assistant, content: reply },
            ]);
        }
        let skip = turns.len().saturating_sub(HISTORY_TURNS);
        Ok(turns.into_iter().skip(skip).flatten().collect())
    }

    // Model calls ---------------------------------------------------------------------------------

    async fn ask_json<T: DeserializeOwned>(
        &self,
        task: Task,
        system: String,
        messages: Vec<ChatMessage>,
        name: &str,
        schema: Value,
    ) -> Result<T, AnalystError> {
        let provider = self.models.as_ref().ok_or(LlmError::NoModel)?.provider(task, &self.routing).await?;
        let mut all = vec![ChatMessage { role: Role::System, content: system }];
        all.extend(messages);
        let response = provider
            .chat(
                ChatRequest {
                    messages: all,
                    temperature: prompts::SQL_TEMPERATURE,
                    max_tokens: prompts::SQL_MAX_TOKENS,
                    format: ResponseFormat::JsonSchema { name: name.to_owned(), schema },
                },
                None,
            )
            .await?;
        serde_json::from_str(response.content.trim())
            .map_err(|error| AnalystError::Failed(format!("The AI model returned an unreadable answer ({error}). Try again.")))
    }

    async fn narrate(&mut self, facts: Value) -> Result<(), AnalystError> {
        if self.score_only {
            return Ok(());
        }
        self.load_model(Task::Narrative).await?;
        self.step("summary", "Writing the answer");
        let provider = self.models.as_ref().ok_or(LlmError::NoModel)?.provider(Task::Narrative, &self.routing).await?;
        let request = |system: String| ChatRequest {
            messages: vec![
                ChatMessage { role: Role::System, content: system },
                ChatMessage { role: Role::User, content: prompts::narrative_user(&self.question, &facts) },
            ],
            temperature: prompts::NARRATIVE_TEMPERATURE,
            max_tokens: prompts::NARRATIVE_MAX_TOKENS,
            format: ResponseFormat::Text,
        };
        let (tokens, mut received) = mpsc::channel::<String>(64);
        let sink = self.sink.clone();
        let forward = async move {
            let mut text = String::new();
            while let Some(delta) = received.recv().await {
                text.push_str(&delta);
                sink(AnalystEvent::Token { text: delta });
            }
            text
        };
        let (response, text) = tokio::join!(provider.chat(request(prompts::NARRATIVE_SYSTEM.to_owned()), Some(tokens)), forward);
        response?;
        let mut content = text.trim().to_owned();
        let mut missing = numcheck::untraced(&content, &facts, &self.question);
        if !missing.is_empty() {
            // The model did arithmetic or invented a figure. Ask once more, strictly, without streaming.
            let strict = format!(
                "{}\nYour previous draft used numbers that are not in FACTS: {}. Rewrite the answer and copy every number exactly from FACTS. \
                 If a figure is not in FACTS, describe it in words instead.",
                prompts::NARRATIVE_SYSTEM,
                missing.join(", ")
            );
            if let Ok(retry) = provider.chat(request(strict), None).await {
                let retried = retry.content.trim().to_owned();
                if !retried.is_empty() {
                    missing = numcheck::untraced(&retried, &facts, &self.question);
                    content = retried;
                }
            }
        }
        if !missing.is_empty() {
            self.answer.assumptions.push(format!(
                "Some figures in the written summary could not be traced to the computed results ({}). Check them against the table.",
                missing.join(", ")
            ));
        }
        self.content = content;
        self.finish_step("summary", StepStatus::Done, None);
        Ok(())
    }

    // Queries -------------------------------------------------------------------------------------

    /// Guards and runs one query, recording it either way. The inner error is a query problem
    /// the model can repair; the outer error ends the answer.
    async fn execute_once(
        &mut self,
        purpose: QueryPurpose,
        sql: &str,
    ) -> Result<Result<(QueryRecord, QueryResult), String>, AnalystError> {
        self.attempts += 1;
        let started = Instant::now();
        let mut record = QueryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            message_id: self.message_id.clone(),
            purpose,
            attempt: self.attempts,
            sql: sql.trim().to_owned(),
            relations: Vec::new(),
            status: QueryStatus::Denied,
            row_count: None,
            duration_ms: None,
            error: None,
            result: None,
            created_at: String::new(),
        };
        let outcome = match sql_guard::validate(sql, &self.policy) {
            Err(violation) => Err(violation.to_string()),
            Ok(validated) => {
                record.sql = validated.sql.clone();
                record.relations = validated.relations.clone();
                let lineage = self.answer.lineage.get_or_insert_with(Lineage::default);
                for column in &validated.columns {
                    if !lineage.columns.contains(&column.name) {
                        lineage.columns.push(column.name.clone());
                    }
                }
                let (workspace, cancel) = (self.workspace.clone(), self.cancel.clone());
                let ran = blocking(move || {
                    let conn = workspace.query_connection()?;
                    Ok(query::run(&conn, &validated, &HashMap::new(), &cancel))
                })
                .await?;
                match ran {
                    Ok(result) => Ok(result),
                    Err(QueryError::Cancelled) => return Err(AnalystError::Cancelled),
                    Err(error) => {
                        record.status = QueryStatus::Error;
                        Err(error.to_string())
                    }
                }
            }
        };
        record.duration_ms = Some(elapsed_ms(started));
        let returned = match outcome {
            Ok(result) => {
                record.status = QueryStatus::Ok;
                record.row_count = Some(result.rows.len() as i64);
                record.result = Some(snapshot(&result));
                Ok(result)
            }
            Err(message) => {
                record.error = Some(message.clone());
                Err(message)
            }
        };
        if !self.score_only {
            self.persist(record.clone()).await?;
        }
        if let Ok(result) = &returned {
            if matches!(purpose, QueryPurpose::Answer | QueryPurpose::Comparison) {
                self.captured = Some((record.sql.clone(), result.clone()));
            }
        }
        Ok(returned.map(|result| (record, result)))
    }

    async fn persist(&self, record: QueryRecord) -> Result<(), AnalystError> {
        let (workspace, question, conversation) = (self.workspace.clone(), self.question.clone(), self.conversation_id.clone());
        blocking(move || {
            let db = workspace.db()?;
            db.insert_query(&record)?;
            db.record_audit_lineage(
                &AuditEntry {
                    kind: "analyst.query",
                    status: match record.status {
                        QueryStatus::Ok => AuditStatus::Ok,
                        QueryStatus::Error => AuditStatus::Error,
                        QueryStatus::Denied => AuditStatus::Denied,
                    },
                    duration_ms: record.duration_ms,
                    query: AuditQuery { question: Some(&question), sql: Some(&record.sql), rows: record.row_count },
                    detail: json!({ "conversation": conversation, "message": record.message_id, "error": record.error }),
                },
                &record.relations,
            )?;
            if !record.relations.is_empty() {
                db.replace_query_relations("query", &record.id, &record.relations)?;
            }
            Ok::<_, crate::error::AppError>(())
        })
        .await?;
        Ok(())
    }

    /// Runs the model's query, feeding errors back to it up to [`MAX_REPAIRS`] times.
    async fn answer_query(
        &mut self,
        context: &SchemaContext,
        mut sql: String,
        suffix: &str,
    ) -> Result<(QueryRecord, QueryResult), AnalystError> {
        let step = format!("run{suffix}");
        self.step(&step, "Running the query");
        let mut repairs = 0;
        loop {
            match self.execute_once(QueryPurpose::Answer, &sql).await? {
                Ok(found) => {
                    let rows = found.1.rows.len();
                    self.finish_step(&step, StepStatus::Done, Some(format!("{rows} {}", if rows == 1 { "row" } else { "rows" })));
                    return Ok(found);
                }
                Err(error) if repairs < MAX_REPAIRS => {
                    repairs += 1;
                    self.step(&step, format!("Fixing the query (attempt {repairs} of {MAX_REPAIRS})"));
                    let repaired: prompts::Repaired = self
                        .ask_json(
                            Task::Sql,
                            prompts::repair_system(&context.prompt, &self.guidance),
                            vec![ChatMessage { role: Role::User, content: prompts::repair_user(&self.question, &sql, &error) }],
                            "repair",
                            prompts::repair_schema(),
                        )
                        .await?;
                    self.answer.explanation = Some(repaired.explanation);
                    sql = repaired.sql;
                }
                Err(error) => {
                    return Err(AnalystError::Failed(format!("The query still failed after {MAX_REPAIRS} fixes: {error}")));
                }
            }
        }
    }

    /// When code checks flag the result, asks the model once whether it really answers the question.
    async fn sanity_check(
        &mut self,
        context: &SchemaContext,
        record: QueryRecord,
        result: QueryResult,
        suffix: &str,
    ) -> Result<(QueryRecord, QueryResult), AnalystError> {
        let largest = record.relations.iter().filter_map(|relation| self.largest_source_rows.get(relation)).max().copied();
        let warnings = facts::sanity_warnings(&record.sql, &result, largest);
        if warnings.is_empty() {
            return Ok((record, result));
        }
        let step = format!("check{suffix}");
        self.step(&step, "Double-checking the result");
        let mut rows = facts::compute(&result).preview;
        context::fence_preview(&mut rows);
        let preview = serde_json::to_string(&rows).map_err(AppError::from)?;
        let verdict: prompts::SelfCheck = self
            .ask_json(
                Task::Sql,
                prompts::self_check_system(&context.prompt, &self.guidance),
                vec![ChatMessage {
                    role: Role::User,
                    content: prompts::self_check_user(&self.question, &record.sql, &warnings, &preview),
                }],
                "self_check",
                prompts::self_check_schema(),
            )
            .await?;
        if verdict.plausible || verdict.sql.trim().is_empty() {
            self.finish_step(&step, StepStatus::Done, Some(verdict.reason));
            self.answer.warnings = warnings;
            return Ok((record, result));
        }
        // `execute_once` captures its result; only keep that when the fix is accepted.
        let original_capture = self.captured.clone();
        if let Ok((fixed, fixed_result)) = self.execute_once(QueryPurpose::Answer, &verdict.sql).await? {
            let remaining = facts::sanity_warnings(&fixed.sql, &fixed_result, largest);
            if remaining.len() < warnings.len() {
                self.finish_step(&step, StepStatus::Done, Some(verdict.reason));
                self.answer.warnings = remaining;
                return Ok((fixed, fixed_result));
            }
        }
        self.captured = original_capture;
        self.finish_step(&step, StepStatus::Done, Some("Kept the original query.".to_owned()));
        self.answer.warnings = warnings;
        Ok((record, result))
    }
}
