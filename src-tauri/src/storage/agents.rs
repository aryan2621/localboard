//! Analysts and what they know: rules (versioned), examples, memories and document chunks.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::workspace_db::WorkspaceDb;
use crate::error::{AppError, AppResult};
use crate::llm::router::TaskRouting;

#[derive(Clone, Copy, Debug)]
pub enum EmbedTarget {
    Rule,
    Example,
    Memory,
    Chunk,
}

#[derive(Clone, Debug)]
pub struct PendingEmbed {
    pub target: EmbedTarget,
    pub id: String,
    pub text: String,
}

/// A piece of an analyst's knowledge used to pick which analyst answers.
#[derive(Clone, Debug)]
pub struct RoutingText {
    pub agent_id: String,
    pub text: String,
    pub embedding: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub model_overrides: TaskRouting,
    pub assigned_tables: Vec<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSnapshot {
    pub title: String,
    pub body: String,
    pub table_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRecord {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub body: String,
    pub table_name: Option<String>,
    pub updated_at: String,
    /// Latest save that has not been promoted. The analyst keeps using `body` until this is.
    pub pending: Option<RuleSnapshot>,
    /// False when this rule has never been promoted, so the analyst ignores it.
    pub promoted: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleVersionRecord {
    pub id: String,
    pub snapshot: RuleSnapshot,
    pub eval_score: Option<f64>,
    pub promoted_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExampleRecord {
    pub id: String,
    pub agent_id: String,
    pub question: String,
    pub sql: String,
    pub origin: String,
    pub message_id: Option<String>,
    pub created_at: String,
    #[serde(skip)]
    pub embedding: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    pub id: String,
    pub agent_id: String,
    pub body: String,
    pub status: String,
    pub source_message_id: Option<String>,
    pub updated_at: String,
    #[serde(skip)]
    pub embedding: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeFileRecord {
    pub id: String,
    pub name: String,
    pub chunk_count: i64,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct ChunkRecord {
    pub file_name: String,
    pub ordinal: i64,
    pub body: String,
    pub embedding: Option<Vec<u8>>,
}

fn now_sql() -> &'static str {
    "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
}

impl WorkspaceDb {
    pub fn list_agents(&self) -> AppResult<Vec<AgentRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, description, model_overrides_json, assigned_tables_json, created_at
             FROM agents ORDER BY created_at, name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| row.map_err(AppError::from).and_then(decode_agent)).collect()
    }

    pub fn agent(&self, id: &str) -> AppResult<AgentRecord> {
        self.list_agents()?.into_iter().find(|agent| agent.id == id).ok_or_else(|| AppError::not_found("Analyst not found"))
    }

    pub fn insert_agent(&self, id: &str, name: &str, description: &str) -> AppResult<AgentRecord> {
        self.conn.execute(
            "INSERT INTO agents(id, name, description) VALUES (?1, ?2, ?3)",
            params![id, name, description],
        )?;
        self.agent(id)
    }

    pub fn update_agent(
        &self,
        id: &str,
        name: &str,
        description: &str,
        overrides: &TaskRouting,
        tables: &[String],
    ) -> AppResult<AgentRecord> {
        let changed = self.conn.execute(
            &format!(
                "UPDATE agents SET name = ?1, description = ?2, model_overrides_json = ?3,
                 assigned_tables_json = ?4, updated_at = {now} WHERE id = ?5",
                now = now_sql()
            ),
            params![name, description, serde_json::to_string(overrides)?, serde_json::to_string(tables)?, id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Analyst not found"));
        }
        self.agent(id)
    }

    pub fn delete_agent(&self, id: &str) -> AppResult<()> {
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM agents", [], |row| row.get(0))?;
        if count <= 1 {
            return Err(AppError::invalid("Keep at least one analyst. Create another before deleting this one."));
        }
        if self.conn.execute("DELETE FROM agents WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Analyst not found"));
        }
        Ok(())
    }

    pub fn list_rules(&self, agent_id: &str) -> AppResult<Vec<RuleRecord>> {
        let rules = self.rules_with_pending(agent_id)?;
        if rules.iter().any(|rule| rule.pending.is_some()) {
            // Edits held by the retired eval gate go live now so no workspace is left with stuck rules.
            self.promote_pending(agent_id)?;
            return self.rules_with_pending(agent_id);
        }
        Ok(rules)
    }

    fn rules_with_pending(&self, agent_id: &str) -> AppResult<Vec<RuleRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, agent_id, title, body, table_name, updated_at
             FROM rules WHERE agent_id = ?1 ORDER BY updated_at, title",
        )?;
        let rows = statement.query_map([agent_id], map_rule)?;
        let rules = rows.collect::<Result<Vec<_>, _>>()?;
        rules.into_iter().map(|rule| self.decorate(rule)).collect()
    }

    /// Rules the analyst may use: the last promoted wording of each rule.
    pub fn list_live_rules(&self, agent_id: &str) -> AppResult<Vec<RuleRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, agent_id, title, body, table_name, updated_at FROM rules
             WHERE agent_id = ?1
             AND EXISTS (SELECT 1 FROM rule_versions v WHERE v.rule_id = rules.id AND v.promoted_at IS NOT NULL)
             ORDER BY updated_at, title",
        )?;
        let rows = statement.query_map([agent_id], map_rule)?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    /// Copies every held save onto the live rule.
    fn promote_pending(&self, agent_id: &str) -> AppResult<Vec<String>> {
        let rules = self.rules_with_pending(agent_id)?;
        let mut promoted = Vec::new();
        for rule in rules {
            let Some(pending) = rule.pending else { continue };
            self.conn.execute(
                &format!(
                    "UPDATE rules SET title = ?1, body = ?2, table_name = ?3, content_hash = ?4, embedding = NULL,
                     updated_at = {now} WHERE id = ?5",
                    now = now_sql()
                ),
                params![pending.title, pending.body, pending.table_name, crate::retrieval::content_hash(&pending.body), rule.id],
            )?;
            self.conn.execute(
                &format!(
                    "UPDATE rule_versions SET promoted_at = {now}
                     WHERE id = (
                       SELECT id FROM rule_versions WHERE rule_id = ?1 AND promoted_at IS NULL ORDER BY created_at DESC LIMIT 1
                     )",
                    now = now_sql()
                ),
                params![rule.id],
            )?;
            promoted.push(rule.id);
        }
        Ok(promoted)
    }

    pub fn insert_rule(
        &self,
        id: &str,
        version_id: &str,
        agent_id: &str,
        snapshot: &RuleSnapshot,
        hash: &str,
        embedding: Option<&[u8]>,
    ) -> AppResult<RuleRecord> {
        self.require_agent(agent_id)?;
        self.conn.execute(
            "INSERT INTO rules(id, agent_id, title, body, table_name, embedding, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, agent_id, snapshot.title, snapshot.body, snapshot.table_name, embedding, hash],
        )?;
        self.insert_version(version_id, id, snapshot)?;
        self.rule(id)
    }

    pub fn update_rule(
        &self,
        id: &str,
        version_id: &str,
        snapshot: &RuleSnapshot,
        hash: &str,
        embedding: Option<&[u8]>,
    ) -> AppResult<RuleRecord> {
        self.rule(id)?;
        let changed = self.conn.execute(
            &format!(
                "UPDATE rules SET title = ?1, body = ?2, table_name = ?3, content_hash = ?4, embedding = ?5,
                 updated_at = {now} WHERE id = ?6",
                now = now_sql()
            ),
            params![snapshot.title, snapshot.body, snapshot.table_name, hash, embedding, id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Rule not found"));
        }
        self.insert_version(version_id, id, snapshot)?;
        self.rule(id)
    }

    /// Puts an older snapshot back as a new version that goes live at once.
    pub fn restore_rule(
        &self,
        id: &str,
        version_id: &str,
        snapshot: &RuleSnapshot,
        hash: &str,
        embedding: Option<&[u8]>,
    ) -> AppResult<RuleRecord> {
        self.update_rule(id, version_id, snapshot, hash, embedding)
    }

    pub fn delete_rule(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM rules WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Rule not found"));
        }
        Ok(())
    }

    pub fn rule_versions(&self, rule_id: &str) -> AppResult<Vec<RuleVersionRecord>> {
        self.rule(rule_id)?;
        let mut statement = self.conn.prepare(
            "SELECT id, snapshot_json, eval_score, promoted_at, created_at FROM rule_versions WHERE rule_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([rule_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get(2)?, row.get(3)?, row.get::<_, String>(4)?))
        })?;
        rows.map(|row| {
            let (id, snapshot, eval_score, promoted_at, created_at) = row?;
            Ok(RuleVersionRecord { id, snapshot: serde_json::from_str(&snapshot)?, eval_score, promoted_at, created_at })
        })
        .collect()
    }

    pub fn rule_version_snapshot(&self, rule_id: &str, version_id: &str) -> AppResult<(RuleSnapshot, Option<f64>)> {
        let found: Option<(String, Option<f64>)> = self
            .conn
            .query_row(
                "SELECT snapshot_json, eval_score FROM rule_versions WHERE id = ?1 AND rule_id = ?2",
                params![version_id, rule_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (json, score) = found.ok_or_else(|| AppError::not_found("That version no longer exists"))?;
        Ok((serde_json::from_str(&json)?, score))
    }

    pub fn list_examples(&self, agent_id: &str) -> AppResult<Vec<ExampleRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, agent_id, question, sql, origin, message_id, created_at, embedding
             FROM examples WHERE agent_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([agent_id], |row| {
            Ok(ExampleRecord {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                question: row.get(2)?,
                sql: row.get(3)?,
                origin: row.get(4)?,
                message_id: row.get(5)?,
                created_at: row.get(6)?,
                embedding: row.get(7)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn example_for_message(&self, message_id: &str) -> AppResult<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM examples WHERE message_id = ?1", [message_id], |row| row.get(0))
            .optional()?)
    }

    pub fn insert_example(
        &self,
        id: &str,
        agent_id: &str,
        question: &str,
        sql: &str,
        origin: &str,
        message_id: Option<&str>,
        hash: &str,
        embedding: Option<&[u8]>,
    ) -> AppResult<ExampleRecord> {
        self.require_agent(agent_id)?;
        self.conn.execute(
            "INSERT INTO examples(id, agent_id, question, sql, origin, message_id, content_hash, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, agent_id, question, sql, origin, message_id, hash, embedding],
        )?;
        self.list_examples(agent_id)?
            .into_iter()
            .find(|example| example.id == id)
            .ok_or_else(|| AppError::Internal("the example was saved but could not be read back".into()))
    }

    pub fn delete_example(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM examples WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Example not found"));
        }
        Ok(())
    }

    pub fn list_memories(&self, agent_id: &str) -> AppResult<Vec<MemoryRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, agent_id, body, status, source_message_id, updated_at, embedding
             FROM memories WHERE agent_id = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([agent_id], memory_row)?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn find_open_memory(&self, agent_id: &str, hash: &str) -> AppResult<Option<MemoryRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, agent_id, body, status, source_message_id, updated_at, embedding
             FROM memories WHERE agent_id = ?1 AND content_hash = ?2 AND status IN ('proposed', 'active') LIMIT 1",
        )?;
        statement.query_row(params![agent_id, hash], memory_row).optional().map_err(AppError::from)
    }

    pub fn insert_memory(
        &self,
        id: &str,
        agent_id: &str,
        body: &str,
        status: &str,
        source_message_id: Option<&str>,
        hash: &str,
        embedding: Option<&[u8]>,
    ) -> AppResult<MemoryRecord> {
        self.require_agent(agent_id)?;
        self.conn.execute(
            "INSERT INTO memories(id, agent_id, body, status, source_message_id, content_hash, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, agent_id, body, status, source_message_id, hash, embedding],
        )?;
        self.memory(id)
    }

    pub fn update_memory(&self, id: &str, body: &str, hash: &str, embedding: Option<&[u8]>) -> AppResult<MemoryRecord> {
        let changed = self.conn.execute(
            &format!(
                "UPDATE memories SET body = ?1, content_hash = ?2, embedding = ?3, updated_at = {now} WHERE id = ?4",
                now = now_sql()
            ),
            params![body, hash, embedding, id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Memory not found"));
        }
        self.memory(id)
    }

    pub fn set_memory_status(&self, id: &str, status: &str) -> AppResult<MemoryRecord> {
        let changed = self.conn.execute(
            &format!("UPDATE memories SET status = ?1, updated_at = {now} WHERE id = ?2", now = now_sql()),
            params![status, id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Memory not found"));
        }
        self.memory(id)
    }

    pub fn delete_memory(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM memories WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Memory not found"));
        }
        Ok(())
    }

    pub fn list_knowledge(&self, agent_id: &str) -> AppResult<Vec<KnowledgeFileRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT f.id, f.name, f.created_at, COUNT(c.id)
             FROM knowledge_files f LEFT JOIN knowledge_chunks c ON c.file_id = f.id
             WHERE f.agent_id = ?1 GROUP BY f.id ORDER BY f.created_at DESC",
        )?;
        let rows = statement.query_map([agent_id], |row| {
            Ok(KnowledgeFileRecord { id: row.get(0)?, name: row.get(1)?, created_at: row.get(2)?, chunk_count: row.get(3)? })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn insert_knowledge(
        &self,
        file_id: &str,
        agent_id: &str,
        name: &str,
        chunks: &[(String, String, String)],
    ) -> AppResult<KnowledgeFileRecord> {
        self.require_agent(agent_id)?;
        self.conn.execute("INSERT INTO knowledge_files(id, agent_id, name) VALUES (?1, ?2, ?3)", params![file_id, agent_id, name])?;
        for (ordinal, (id, body, hash)) in chunks.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO knowledge_chunks(id, file_id, agent_id, ordinal, body, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, file_id, agent_id, ordinal as i64, body, hash],
            )?;
        }
        Ok(KnowledgeFileRecord { id: file_id.to_owned(), name: name.to_owned(), chunk_count: chunks.len() as i64, created_at: String::new() })
    }

    pub fn delete_knowledge(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM knowledge_files WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Document not found"));
        }
        Ok(())
    }

    pub fn chunks_for_agent(&self, agent_id: &str) -> AppResult<Vec<ChunkRecord>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT f.name, c.ordinal, c.body, c.embedding FROM knowledge_chunks c
                 JOIN knowledge_files f ON f.id = c.file_id WHERE c.agent_id = ?1 ORDER BY c.file_id, c.ordinal",
            )?;
        let rows = statement.query_map([agent_id], |row| Ok(ChunkRecord { file_name: row.get(0)?, ordinal: row.get(1)?, body: row.get(2)?, embedding: row.get(3)? }))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn routing_texts(&self) -> AppResult<Vec<RoutingText>> {
        let mut texts = Vec::new();
        let mut statement = self.conn.prepare(
            "SELECT r.agent_id, r.title || ' ' || r.body, r.embedding FROM rules r
             WHERE EXISTS (SELECT 1 FROM rule_versions v WHERE v.rule_id = r.id AND v.promoted_at IS NOT NULL)",
        )?;
        for row in statement.query_map([], routing_row)? {
            texts.push(row?);
        }
        let mut statement = self.conn.prepare("SELECT agent_id, question, embedding FROM examples")?;
        for row in statement.query_map([], routing_row)? {
            texts.push(row?);
        }
        let mut statement = self.conn.prepare("SELECT agent_id, body, embedding FROM memories WHERE status = 'active'")?;
        for row in statement.query_map([], routing_row)? {
            texts.push(row?);
        }
        Ok(texts)
    }

    /// Texts saved before an embedding model was available, oldest gaps first, capped.
    pub fn pending_embeddings(&self, agent_id: &str, limit: usize) -> AppResult<Vec<PendingEmbed>> {
        let mut pending = Vec::new();
        let queries: [(&str, EmbedTarget); 4] = [
            ("SELECT id, body FROM rules WHERE agent_id = ?1 AND embedding IS NULL", EmbedTarget::Rule),
            ("SELECT id, question FROM examples WHERE agent_id = ?1 AND embedding IS NULL", EmbedTarget::Example),
            ("SELECT id, body FROM memories WHERE agent_id = ?1 AND status = 'active' AND embedding IS NULL", EmbedTarget::Memory),
            ("SELECT id, body FROM knowledge_chunks WHERE agent_id = ?1 AND embedding IS NULL", EmbedTarget::Chunk),
        ];
        for (sql, target) in queries {
            if pending.len() >= limit {
                break;
            }
            let mut statement = self.conn.prepare(sql)?;
            let rows = statement.query_map(params![agent_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, text) = row?;
                pending.push(PendingEmbed { target, id, text });
                if pending.len() >= limit {
                    break;
                }
            }
        }
        Ok(pending)
    }

    pub fn store_embedding(&self, target: EmbedTarget, id: &str, blob: &[u8]) -> AppResult<()> {
        let table = match target {
            EmbedTarget::Rule => "rules",
            EmbedTarget::Example => "examples",
            EmbedTarget::Memory => "memories",
            EmbedTarget::Chunk => "knowledge_chunks",
        };
        self.conn.execute(&format!("UPDATE {table} SET embedding = ?1 WHERE id = ?2"), params![blob, id])?;
        Ok(())
    }

    fn require_agent(&self, id: &str) -> AppResult<()> {
        let found: Option<String> = self.conn.query_row("SELECT id FROM agents WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        found.map(|_| ()).ok_or_else(|| AppError::not_found("Analyst not found"))
    }

    fn rule(&self, id: &str) -> AppResult<RuleRecord> {
        let found = self
            .conn
            .query_row(
                "SELECT id, agent_id, title, body, table_name, updated_at FROM rules WHERE id = ?1",
                [id],
                map_rule,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Rule not found"))?;
        self.decorate(found)
    }

    fn decorate(&self, mut rule: RuleRecord) -> AppResult<RuleRecord> {
        let promoted: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM rule_versions WHERE rule_id = ?1 AND promoted_at IS NOT NULL",
            [&rule.id],
            |row| row.get(0),
        )?;
        rule.promoted = promoted > 0;
        let latest: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT snapshot_json, promoted_at FROM rule_versions WHERE rule_id = ?1 ORDER BY created_at DESC LIMIT 1",
                [&rule.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((json, promoted_at)) = latest {
            if promoted_at.is_none() {
                rule.pending = Some(serde_json::from_str(&json)?);
            }
        }
        Ok(rule)
    }

    fn memory(&self, id: &str) -> AppResult<MemoryRecord> {
        self.conn
            .query_row(
                "SELECT id, agent_id, body, status, source_message_id, updated_at, embedding FROM memories WHERE id = ?1",
                [id],
                memory_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Memory not found"))
    }

    fn insert_version(&self, version_id: &str, rule_id: &str, snapshot: &RuleSnapshot) -> AppResult<()> {
        self.conn.execute(
            &format!(
                "INSERT INTO rule_versions(id, rule_id, snapshot_json, promoted_at) VALUES (?1, ?2, ?3, {now})",
                now = now_sql()
            ),
            params![version_id, rule_id, serde_json::to_string(snapshot)?],
        )?;
        Ok(())
    }
}

fn map_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuleRecord> {
    Ok(RuleRecord {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        table_name: row.get(4)?,
        updated_at: row.get(5)?,
        pending: None,
        promoted: false,
    })
}

fn memory_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    Ok(MemoryRecord {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        body: row.get(2)?,
        status: row.get(3)?,
        source_message_id: row.get(4)?,
        updated_at: row.get(5)?,
        embedding: row.get(6)?,
    })
}

fn routing_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RoutingText> {
    Ok(RoutingText { agent_id: row.get(0)?, text: row.get(1)?, embedding: row.get(2)? })
}

fn decode_agent(row: (String, String, String, String, String, String)) -> AppResult<AgentRecord> {
    let (id, name, description, overrides, tables, created_at) = row;
    Ok(AgentRecord {
        id,
        name,
        description,
        model_overrides: serde_json::from_str(&overrides)?,
        assigned_tables: serde_json::from_str(&tables)?,
        created_at,
    })
}
