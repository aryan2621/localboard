use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::workspace_db::WorkspaceDb;
use crate::engine::query::QueryResult;
use crate::error::{AppError, AppResult};

/// Rows kept with each query so answers render after a restart without re-running anything.
pub const RESULT_SNAPSHOT_ROWS: usize = 1_000;
const CONVERSATION_LIST_LIMIT: i64 = 200;
const TITLE_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Pending,
    Done,
    Error,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryPurpose {
    Answer,
    Comparison,
    Breakdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryStatus {
    Ok,
    Error,
    Denied,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub seq: i64,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub answer: Option<Value>,
    pub feedback: Option<i8>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRecord {
    pub id: String,
    pub message_id: String,
    pub purpose: QueryPurpose,
    pub attempt: i64,
    pub sql: String,
    pub relations: Vec<String>,
    pub status: QueryStatus,
    pub row_count: Option<i64>,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
    pub result: Option<QueryResult>,
    pub created_at: String,
}

fn text<T: Serialize>(value: &T) -> AppResult<String> {
    let json = serde_json::to_value(value)?;
    json.as_str().map(str::to_owned).ok_or_else(|| AppError::Internal("expected a string enum".into()))
}

fn parse<T: for<'de> Deserialize<'de>>(raw: &str) -> AppResult<T> {
    Ok(serde_json::from_value(Value::String(raw.to_owned()))?)
}

/// Keeps the first rows of a result for display; the full result is always re-runnable.
pub fn snapshot(result: &QueryResult) -> QueryResult {
    let keep = result.rows.len().min(RESULT_SNAPSHOT_ROWS);
    QueryResult {
        columns: result.columns.clone(),
        rows: result.rows[..keep].to_vec(),
        truncated: result.truncated || keep < result.rows.len(),
    }
}

/// Turns free text into an FTS5 prefix query of quoted terms, so user input is never FTS syntax.
fn fts_query(search: &str) -> Option<String> {
    let terms: Vec<String> = search
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(8)
        .map(|term| format!("\"{term}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

pub fn title_from(question: &str) -> String {
    let flat = question.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= TITLE_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(TITLE_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

const MESSAGE_COLUMNS: &str = "id, conversation_id, seq, role, content, status, answer_json, feedback, created_at";
const QUERY_COLUMNS: &str =
    "q.id, q.message_id, q.purpose, q.attempt, q.sql, q.relations_json, q.status, q.row_count, q.duration_ms, q.error, q.result_json, q.created_at";

type RawMessage = (String, String, i64, String, String, String, Option<String>, Option<i8>, String);
type RawQuery = (
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
);

fn raw_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMessage> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?))
}

fn decode_message(raw: RawMessage) -> AppResult<MessageRecord> {
    let (id, conversation_id, seq, role, content, status, answer, feedback, created_at) = raw;
    Ok(MessageRecord {
        id,
        conversation_id,
        seq,
        role: parse(&role)?,
        content,
        status: parse(&status)?,
        answer: answer.map(|json| serde_json::from_str(&json)).transpose()?,
        feedback,
        created_at,
    })
}

fn raw_query(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawQuery> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn decode_query(raw: RawQuery) -> AppResult<QueryRecord> {
    let (id, message_id, purpose, attempt, sql, relations, status, row_count, duration_ms, error, result, created_at) = raw;
    Ok(QueryRecord {
        id,
        message_id,
        purpose: parse(&purpose)?,
        attempt,
        sql,
        relations: serde_json::from_str(&relations)?,
        status: parse(&status)?,
        row_count,
        duration_ms,
        error,
        result: result.map(|json| serde_json::from_str(&json)).transpose()?,
        created_at,
    })
}

impl WorkspaceDb {
    /// Answers that were being written when LocalBoard stopped can never finish.
    pub(super) fn fail_interrupted_answers(&self) -> AppResult<()> {
        self.conn.execute(
            "UPDATE messages SET status = 'error', content = 'This answer was interrupted when LocalBoard closed.'
             WHERE status = 'pending'",
            [],
        )?;
        Ok(())
    }

    // Conversations -----------------------------------------------------------------------------

    pub fn create_conversation(&self, id: &str, title: &str) -> AppResult<()> {
        self.conn.execute("INSERT INTO conversations(id, title) VALUES (?1, ?2)", params![id, title])?;
        Ok(())
    }

    pub fn conversation_title(&self, id: &str) -> AppResult<String> {
        self.conn
            .query_row("SELECT title FROM conversations WHERE id = ?1", [id], |row| row.get(0))
            .optional()?
            .ok_or_else(|| AppError::not_found("Conversation not found"))
    }

    /// Most recent first; with `search`, only conversations whose messages match it.
    pub fn list_conversations(&self, search: Option<&str>) -> AppResult<Vec<ConversationSummary>> {
        let map = |row: &rusqlite::Row<'_>| Ok(ConversationSummary { id: row.get(0)?, title: row.get(1)?, updated_at: row.get(2)? });
        let rows: Vec<ConversationSummary> = match search.and_then(fts_query) {
            Some(query) => {
                let mut statement = self.conn.prepare(
                    "SELECT c.id, c.title, c.updated_at FROM conversations c
                     WHERE c.id IN (SELECT m.conversation_id FROM messages_fts f JOIN messages m ON m.rowid = f.rowid
                                    WHERE messages_fts MATCH ?1)
                     ORDER BY c.updated_at DESC LIMIT ?2",
                )?;
                let rows = statement.query_map(params![query, CONVERSATION_LIST_LIMIT], map)?;
                rows.collect::<Result<_, _>>()?
            }
            None => {
                let mut statement = self
                    .conn
                    .prepare("SELECT id, title, updated_at FROM conversations ORDER BY updated_at DESC LIMIT ?1")?;
                let rows = statement.query_map([CONVERSATION_LIST_LIMIT], map)?;
                rows.collect::<Result<_, _>>()?
            }
        };
        Ok(rows)
    }

    /// Distinct questions that got an answer, most recent first, for prompt autocomplete.
    pub fn recent_questions(&self, limit: usize) -> AppResult<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT asked.content FROM messages asked
             JOIN messages answered ON answered.conversation_id = asked.conversation_id AND answered.seq = asked.seq + 1
             WHERE asked.role = 'user' AND answered.status = 'done'
             GROUP BY asked.content ORDER BY max(asked.created_at) DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> AppResult<()> {
        if self.conn.execute("UPDATE conversations SET title = ?2 WHERE id = ?1", params![id, title])? == 0 {
            return Err(AppError::not_found("Conversation not found"));
        }
        Ok(())
    }

    pub fn delete_conversation(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM conversations WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Conversation not found"));
        }
        Ok(())
    }

    // Messages ----------------------------------------------------------------------------------

    pub fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: MessageStatus,
    ) -> AppResult<MessageRecord> {
        self.conn.execute(
            "INSERT INTO messages(id, conversation_id, seq, role, content, status)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?2), ?3, ?4, ?5)",
            params![id, conversation_id, text(&role)?, content, text(&status)?],
        )?;
        self.conn.execute(
            "UPDATE conversations SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [conversation_id],
        )?;
        self.message(id)
    }

    pub fn message(&self, id: &str) -> AppResult<MessageRecord> {
        let raw = self
            .conn
            .query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"), [id], raw_message)
            .optional()?
            .ok_or_else(|| AppError::not_found("Message not found"))?;
        decode_message(raw)
    }

    pub fn messages(&self, conversation_id: &str) -> AppResult<Vec<MessageRecord>> {
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1 ORDER BY seq"))?;
        let rows = statement.query_map([conversation_id], raw_message)?;
        rows.map(|row| row.map_err(AppError::from).and_then(decode_message)).collect()
    }

    pub fn finish_message<T: Serialize>(&self, id: &str, content: &str, status: MessageStatus, answer: &T) -> AppResult<()> {
        self.conn.execute(
            "UPDATE messages SET content = ?1, status = ?2, answer_json = ?3 WHERE id = ?4",
            params![content, text(&status)?, serde_json::to_string(answer)?, id],
        )?;
        Ok(())
    }

    pub fn set_feedback(&self, message_id: &str, feedback: Option<i8>) -> AppResult<()> {
        let changed = self.conn.execute(
            "UPDATE messages SET feedback = ?1 WHERE id = ?2 AND role = 'assistant'",
            params![feedback, message_id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Answer not found"));
        }
        Ok(())
    }

    // Queries -----------------------------------------------------------------------------------

    pub fn insert_query(&self, record: &QueryRecord) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO queries(id, message_id, purpose, attempt, sql, relations_json, status, row_count, duration_ms, error, result_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.id,
                record.message_id,
                text(&record.purpose)?,
                record.attempt,
                record.sql,
                serde_json::to_string(&record.relations)?,
                text(&record.status)?,
                record.row_count,
                record.duration_ms,
                record.error,
                record.result.as_ref().map(serde_json::to_string).transpose()?
            ],
        )?;
        Ok(())
    }

    /// Stores a fresh run of an existing query (Re-run).
    pub fn update_query_run(&self, record: &QueryRecord) -> AppResult<()> {
        self.conn.execute(
            "UPDATE queries SET status = ?1, row_count = ?2, duration_ms = ?3, error = ?4, result_json = ?5 WHERE id = ?6",
            params![
                text(&record.status)?,
                record.row_count,
                record.duration_ms,
                record.error,
                record.result.as_ref().map(serde_json::to_string).transpose()?,
                record.id
            ],
        )?;
        Ok(())
    }

    pub fn query(&self, id: &str) -> AppResult<QueryRecord> {
        let raw = self
            .conn
            .query_row(&format!("SELECT {QUERY_COLUMNS} FROM queries q WHERE q.id = ?1"), [id], raw_query)
            .optional()?
            .ok_or_else(|| AppError::not_found("Query not found"))?;
        decode_query(raw)
    }

    pub fn conversation_queries(&self, conversation_id: &str) -> AppResult<Vec<QueryRecord>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {QUERY_COLUMNS} FROM queries q JOIN messages m ON m.id = q.message_id
             WHERE m.conversation_id = ?1 ORDER BY m.seq, q.attempt"
        ))?;
        let rows = statement.query_map([conversation_id], raw_query)?;
        rows.map(|row| row.map_err(AppError::from).and_then(decode_query)).collect()
    }
}
