use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, ToSql};
use serde::Serialize;
use serde_json::Value;

use super::migrations;
use crate::connectors::ConnectorKind;
use crate::engine::profile::{ColumnProfile, SampleRow, TableProfile};
use crate::engine::relationships::DetectedRelationship;
use crate::error::{AppError, AppResult};

#[derive(Clone, Debug)]
pub struct ConnectionRecord {
    pub id: String,
    pub kind: ConnectorKind,
    pub name: String,
    pub config: Value,
    pub secret_ref: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TableRecord {
    pub id: String,
    pub connection_id: String,
    pub source_ref: String,
    pub relation: String,
    pub description: String,
    pub row_count: Option<i64>,
    pub profile_error: Option<String>,
    pub profiled_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ColumnRecord {
    pub name: String,
    pub ordinal: i64,
    pub data_type: String,
    pub description: String,
    pub profile: Option<ColumnProfile>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RelationshipRecord {
    pub id: String,
    pub from_table_id: String,
    pub from_column: String,
    pub to_table_id: String,
    pub to_column: String,
    pub confidence: f64,
    pub origin: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditStatus {
    Ok,
    Error,
    Denied,
}

#[derive(Default)]
pub struct AuditQuery<'a> {
    pub question: Option<&'a str>,
    pub sql: Option<&'a str>,
    pub rows: Option<i64>,
}

pub struct AuditEntry<'a> {
    pub kind: &'a str,
    pub status: AuditStatus,
    pub duration_ms: Option<i64>,
    pub query: AuditQuery<'a>,
    pub detail: Value,
}

#[derive(Default)]
pub struct AuditFilter {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub before_ts: Option<String>,
    pub before_id: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: i64,
    pub ts: String,
    pub actor: String,
    pub kind: String,
    pub question: Option<String>,
    pub sql: Option<String>,
    pub rows: Option<i64>,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub detail: Value,
    pub relations: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRef {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileUsage {
    pub id: String,
    pub name: String,
    pub dashboard_id: String,
    pub dashboard_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactUsage {
    pub id: String,
    pub name: String,
    pub parent_id: String,
    pub parent_name: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableUsage {
    pub last_audit_at: Option<String>,
    pub chats: Vec<UsageRef>,
    pub saved_queries: Vec<UsageRef>,
    pub tiles: Vec<TileUsage>,
    pub automations: Vec<UsageRef>,
    pub datasites: Vec<ArtifactUsage>,
    pub reports: Vec<ArtifactUsage>,
    pub decks: Vec<ArtifactUsage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependent {
    pub kind: String,
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    pub entries: Vec<AuditRow>,
    pub has_more: bool,
}

pub const AUDIT_PAGE: i64 = 100;

const TABLE_COLUMNS: &str =
    "id, connection_id, source_ref, duck_relation, description, row_count, profile_error, profiled_at";

/// One workspace's metadata store. All SQL in this file is parameterized.
pub struct WorkspaceDb {
    pub(super) conn: Connection,
    actor: String,
}

impl WorkspaceDb {
    pub fn open(path: &Path) -> AppResult<Self> {
        Self::open_with_key(path, None)
    }

    pub fn open_with_key(path: &Path, key: Option<&str>) -> AppResult<Self> {
        let mut conn = Connection::open(path)?;
        if let Some(key) = key {
            conn.pragma_update(None, "key", key)?;
        }
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;").map_err(|error| {
            AppError::invalid(format!("Could not open the workspace database. If it is encrypted, the passphrase is wrong. {error}"))
        })?;
        migrations::apply(&mut conn, migrations::WORKSPACE)?;
        let db = Self { conn, actor: "local".into() };
        db.fail_interrupted_answers()?;
        Ok(db)
    }

    pub fn set_actor(&mut self, actor: impl Into<String>) {
        let name = actor.into();
        self.actor = if name.trim().is_empty() { "local".into() } else { name };
    }

    // Connections -------------------------------------------------------------------------------

    /// Returns false when a connection with this id already exists (idempotent imports).
    pub fn insert_connection(&self, record: &ConnectionRecord) -> AppResult<bool> {
        let inserted = self.conn.execute(
            "INSERT INTO connections(id, kind, name, config_json, secret_ref) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO NOTHING",
            params![
                record.id,
                record.kind.as_str(),
                record.name.trim(),
                serde_json::to_string(&record.config)?,
                record.secret_ref
            ],
        )?;
        Ok(inserted == 1)
    }

    pub fn list_connections(&self) -> AppResult<Vec<ConnectionRecord>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, kind, name, config_json, secret_ref, created_at FROM connections ORDER BY created_at, name")?;
        let rows = statement.query_map([], raw_connection)?;
        rows.map(|row| row.map_err(AppError::from).and_then(decode_connection)).collect()
    }

    pub fn get_connection(&self, id: &str) -> AppResult<ConnectionRecord> {
        let raw = self
            .conn
            .query_row(
                "SELECT id, kind, name, config_json, secret_ref, created_at FROM connections WHERE id = ?1",
                [id],
                raw_connection,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Connection not found"))?;
        decode_connection(raw)
    }

    pub fn delete_connection(&self, id: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM connections WHERE id = ?1", [id])?;
        Ok(())
    }

    // Catalog -----------------------------------------------------------------------------------

    pub fn list_tables(&self) -> AppResult<Vec<TableRecord>> {
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {TABLE_COLUMNS} FROM catalog_tables ORDER BY duck_relation"))?;
        let rows = statement.query_map([], map_table)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn tables_for_connection(&self, connection_id: &str) -> AppResult<Vec<TableRecord>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {TABLE_COLUMNS} FROM catalog_tables WHERE connection_id = ?1 ORDER BY duck_relation"
        ))?;
        let rows = statement.query_map([connection_id], map_table)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get_table(&self, id: &str) -> AppResult<TableRecord> {
        self.conn
            .query_row(&format!("SELECT {TABLE_COLUMNS} FROM catalog_tables WHERE id = ?1"), [id], map_table)
            .optional()?
            .ok_or_else(|| AppError::not_found("Table not found"))
    }

    pub fn find_table(&self, connection_id: &str, source_ref: &str) -> AppResult<Option<TableRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {TABLE_COLUMNS} FROM catalog_tables WHERE connection_id = ?1 AND source_ref = ?2"),
                params![connection_id, source_ref],
                map_table,
            )
            .optional()?)
    }

    pub fn relation_taken(&self, relation: &str) -> AppResult<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM catalog_tables WHERE duck_relation = ?1 COLLATE NOCASE",
            [relation],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn insert_table(&self, id: &str, connection_id: &str, source_ref: &str, relation: &str) -> AppResult<TableRecord> {
        self.conn.execute(
            "INSERT INTO catalog_tables(id, connection_id, source_ref, duck_relation) VALUES (?1, ?2, ?3, ?4)",
            params![id, connection_id, source_ref, relation],
        )?;
        self.get_table(id)
    }

    pub fn delete_table(&self, id: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM catalog_tables WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Stores a fresh profile. Column descriptions written by users survive re-profiling.
    pub fn save_profile(&mut self, table_id: &str, profile: &TableProfile) -> AppResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE catalog_tables SET row_count = ?1, samples_json = ?2, profile_error = NULL,
               profiled_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?3",
            params![profile.row_count, serde_json::to_string(&profile.samples)?, table_id],
        )?;
        tx.execute("CREATE TEMP TABLE IF NOT EXISTS profiled_columns(name TEXT PRIMARY KEY)", [])?;
        tx.execute("DELETE FROM temp.profiled_columns", [])?;
        for (ordinal, column) in profile.columns.iter().enumerate() {
            tx.execute(
                "INSERT INTO catalog_columns(table_id, name, ordinal, data_type, profile_json) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(table_id, name) DO UPDATE SET
                   ordinal = excluded.ordinal, data_type = excluded.data_type, profile_json = excluded.profile_json",
                params![table_id, column.name, ordinal as i64, column.data_type, serde_json::to_string(column)?],
            )?;
            tx.execute("INSERT INTO temp.profiled_columns(name) VALUES (?1)", [&column.name])?;
        }
        tx.execute(
            "DELETE FROM catalog_columns WHERE table_id = ?1 AND name NOT IN (SELECT name FROM temp.profiled_columns)",
            [table_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_profile_error(&self, table_id: &str, message: &str) -> AppResult<()> {
        self.conn
            .execute("UPDATE catalog_tables SET profile_error = ?1 WHERE id = ?2", params![message, table_id])?;
        Ok(())
    }

    pub fn samples(&self, table_id: &str) -> AppResult<Vec<SampleRow>> {
        let raw: Option<String> =
            self.conn.query_row("SELECT samples_json FROM catalog_tables WHERE id = ?1", [table_id], |row| row.get(0))?;
        Ok(raw.map(|json| serde_json::from_str(&json)).transpose()?.unwrap_or_default())
    }

    pub fn columns(&self, table_id: &str) -> AppResult<Vec<ColumnRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT name, ordinal, data_type, description, profile_json FROM catalog_columns
             WHERE table_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map([table_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get::<_, Option<String>>(4)?))
        })?;
        rows.map(|row| {
            let (name, ordinal, data_type, description, profile) = row?;
            Ok(ColumnRecord {
                name,
                ordinal,
                data_type,
                description,
                profile: profile.map(|json| serde_json::from_str(&json)).transpose()?,
            })
        })
        .collect()
    }

    pub fn set_table_description(&self, id: &str, description: &str) -> AppResult<()> {
        let changed = self
            .conn
            .execute("UPDATE catalog_tables SET description = ?1 WHERE id = ?2", params![description.trim(), id])?;
        if changed == 0 {
            return Err(AppError::not_found("Table not found"));
        }
        Ok(())
    }

    pub fn set_table_relation(&self, id: &str, relation: &str) -> AppResult<()> {
        self.conn
            .execute("UPDATE catalog_tables SET duck_relation = ?1 WHERE id = ?2", params![relation, id])?;
        Ok(())
    }

    pub fn set_column_description(&self, table_id: &str, column: &str, description: &str) -> AppResult<()> {
        let changed = self.conn.execute(
            "UPDATE catalog_columns SET description = ?1 WHERE table_id = ?2 AND name = ?3",
            params![description.trim(), table_id, column],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Column not found"));
        }
        Ok(())
    }

    pub fn replace_detected_relationships(&mut self, detected: &[DetectedRelationship]) -> AppResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM catalog_relationships WHERE origin = 'detected'", [])?;
        for relationship in detected {
            tx.execute(
                "INSERT INTO catalog_relationships(id, from_table_id, from_column, to_table_id, to_column, confidence, origin)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'detected')
                 ON CONFLICT(from_table_id, from_column, to_table_id, to_column) DO NOTHING",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    relationship.from_table_id,
                    relationship.from_column,
                    relationship.to_table_id,
                    relationship.to_column,
                    relationship.confidence
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn relationships(&self) -> AppResult<Vec<RelationshipRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, from_table_id, from_column, to_table_id, to_column, confidence, origin
             FROM catalog_relationships ORDER BY confidence DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RelationshipRecord {
                id: row.get(0)?,
                from_table_id: row.get(1)?,
                from_column: row.get(2)?,
                to_table_id: row.get(3)?,
                to_column: row.get(4)?,
                confidence: row.get(5)?,
                origin: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // Audit -------------------------------------------------------------------------------------

    pub fn record_audit(&self, entry: &AuditEntry<'_>) -> AppResult<i64> {
        self.conn.execute(
            "INSERT INTO audit_log(actor, kind, question, sql, rows, duration_ms, status, detail_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                self.actor,
                entry.kind,
                entry.query.question,
                entry.query.sql,
                entry.query.rows,
                entry.duration_ms,
                match entry.status {
                    AuditStatus::Ok => "ok",
                    AuditStatus::Error => "error",
                    AuditStatus::Denied => "denied",
                },
                serde_json::to_string(&entry.detail)?
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn try_record_audit(&self, entry: &AuditEntry<'_>) -> Option<i64> {
        match self.record_audit(entry) {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::error!("could not write audit entry {}: {error}", entry.kind);
                None
            }
        }
    }

    pub fn record_audit_lineage(&self, entry: &AuditEntry<'_>, relations: &[String]) -> AppResult<i64> {
        let id = self.record_audit(entry)?;
        self.replace_query_relations("audit", &id.to_string(), relations)?;
        Ok(id)
    }

    pub fn try_record_audit_lineage(&self, entry: &AuditEntry<'_>, relations: &[String]) {
        if let Some(id) = self.try_record_audit(entry) {
            if let Err(error) = self.replace_query_relations("audit", &id.to_string(), relations) {
                tracing::error!("could not write query relations for {}: {error}", entry.kind);
            }
        }
    }

    pub fn replace_query_relations(&self, kind: &str, ref_id: &str, relations: &[String]) -> AppResult<()> {
        self.conn.execute("DELETE FROM query_relations WHERE kind = ?1 AND ref_id = ?2", params![kind, ref_id])?;
        for relation in relations {
            if relation.is_empty() {
                continue;
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO query_relations(kind, ref_id, relation) VALUES (?1, ?2, ?3)",
                params![kind, ref_id, relation],
            )?;
        }
        Ok(())
    }

    pub fn clear_query_relations(&self, kind: &str, ref_id: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM query_relations WHERE kind = ?1 AND ref_id = ?2", params![kind, ref_id])?;
        Ok(())
    }

    pub fn rename_query_relation(&self, from: &str, to: &str) -> AppResult<()> {
        self.conn.execute("UPDATE query_relations SET relation = ?1 WHERE relation = ?2", params![to, from])?;
        Ok(())
    }

    pub fn table_usage(&self, relation: &str) -> AppResult<TableUsage> {
        let last_audit_at = self
            .conn
            .query_row(
                "SELECT MAX(a.ts) FROM query_relations qr
                 JOIN audit_log a ON a.id = CAST(qr.ref_id AS INTEGER)
                 WHERE qr.kind = 'audit' AND qr.relation = ?1",
                [relation],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let chats = {
            let mut statement = self.conn.prepare(
                "SELECT DISTINCT c.id, c.title
                 FROM query_relations qr
                 JOIN queries q ON q.id = qr.ref_id
                 JOIN messages m ON m.id = q.message_id
                 JOIN conversations c ON c.id = m.conversation_id
                 WHERE qr.kind = 'query' AND qr.relation = ?1
                 ORDER BY c.updated_at DESC",
            )?;
            let rows = statement.query_map([relation], |row| Ok(UsageRef { id: row.get(0)?, name: row.get(1)? }))?;
            rows.collect::<Result<_, _>>()?
        };
        let saved_queries = {
            let mut statement = self.conn.prepare(
                "SELECT s.id, s.name FROM query_relations qr
                 JOIN saved_queries s ON s.id = qr.ref_id
                 WHERE qr.kind = 'saved' AND qr.relation = ?1
                 ORDER BY s.name",
            )?;
            let rows = statement.query_map([relation], |row| Ok(UsageRef { id: row.get(0)?, name: row.get(1)? }))?;
            rows.collect::<Result<_, _>>()?
        };
        let tiles = {
            let mut statement = self.conn.prepare(
                "SELECT t.id, t.title, t.dashboard_id, d.name
                 FROM query_relations qr
                 JOIN dashboard_tiles t ON t.id = qr.ref_id
                 JOIN dashboards d ON d.id = t.dashboard_id
                 WHERE qr.kind = 'tile' AND qr.relation = ?1
                 ORDER BY d.name, t.title",
            )?;
            let rows = statement.query_map([relation], |row| {
                Ok(TileUsage { id: row.get(0)?, name: row.get(1)?, dashboard_id: row.get(2)?, dashboard_name: row.get(3)? })
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let automations = {
            let mut statement = self.conn.prepare(
                "SELECT a.id, a.name FROM query_relations qr
                 JOIN automations a ON a.id = qr.ref_id
                 WHERE qr.kind = 'automation' AND qr.relation = ?1
                 ORDER BY a.name",
            )?;
            let rows = statement.query_map([relation], |row| Ok(UsageRef { id: row.get(0)?, name: row.get(1)? }))?;
            rows.collect::<Result<_, _>>()?
        };
        let datasites = {
            let mut statement = self.conn.prepare(
                "SELECT c.id, c.title, s.id, s.name FROM query_relations qr
                 JOIN datasite_charts c ON c.id = qr.ref_id
                 JOIN datasite_pages p ON p.id = c.page_id
                 JOIN datasites s ON s.id = p.datasite_id
                 WHERE qr.kind = 'datasite' AND qr.relation = ?1
                 ORDER BY s.name, c.title",
            )?;
            let rows = statement.query_map([relation], |row| {
                Ok(ArtifactUsage { id: row.get(0)?, name: row.get(1)?, parent_id: row.get(2)?, parent_name: row.get(3)? })
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let reports = {
            let mut statement = self.conn.prepare(
                "SELECT b.id, b.title, r.id, r.name FROM query_relations qr
                 JOIN report_blocks b ON b.id = qr.ref_id
                 JOIN reports r ON r.id = b.report_id
                 WHERE qr.kind = 'report' AND qr.relation = ?1
                 ORDER BY r.name, b.title",
            )?;
            let rows = statement.query_map([relation], |row| {
                Ok(ArtifactUsage { id: row.get(0)?, name: row.get(1)?, parent_id: row.get(2)?, parent_name: row.get(3)? })
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let decks = {
            let mut statement = self.conn.prepare(
                "SELECT s.id, s.title, d.id, d.name FROM query_relations qr
                 JOIN deck_slides s ON s.id = qr.ref_id
                 JOIN decks d ON d.id = s.deck_id
                 WHERE qr.kind = 'deck' AND qr.relation = ?1
                 ORDER BY d.name, s.title",
            )?;
            let rows = statement.query_map([relation], |row| {
                Ok(ArtifactUsage { id: row.get(0)?, name: row.get(1)?, parent_id: row.get(2)?, parent_name: row.get(3)? })
            })?;
            rows.collect::<Result<_, _>>()?
        };
        Ok(TableUsage { last_audit_at, chats, saved_queries, tiles, automations, datasites, reports, decks })
    }

    pub fn dependents_of_relations(&self, relations: &[String]) -> AppResult<Vec<Dependent>> {
        if relations.is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut dependents = Vec::new();
        for relation in relations {
            for item in self.dependents_of(relation)? {
                if seen.insert((item.kind.clone(), item.id.clone())) {
                    dependents.push(item);
                }
            }
        }
        Ok(dependents)
    }

    fn dependents_of(&self, relation: &str) -> AppResult<Vec<Dependent>> {
        let usage = self.table_usage(relation)?;
        let mut items = Vec::new();
        for chat in usage.chats {
            items.push(Dependent { kind: "chat".into(), id: chat.id, name: chat.name });
        }
        for saved in usage.saved_queries {
            items.push(Dependent { kind: "saved".into(), id: saved.id, name: saved.name });
        }
        for tile in usage.tiles {
            items.push(Dependent { kind: "tile".into(), id: tile.id, name: format!("{} · {}", tile.name, tile.dashboard_name) });
        }
        for automation in usage.automations {
            items.push(Dependent { kind: "automation".into(), id: automation.id, name: automation.name });
        }
        for item in usage.datasites {
            items.push(Dependent { kind: "datasite".into(), id: item.parent_id, name: format!("{} · {}", item.name, item.parent_name) });
        }
        for item in usage.reports {
            items.push(Dependent { kind: "report".into(), id: item.parent_id, name: format!("{} · {}", item.name, item.parent_name) });
        }
        for item in usage.decks {
            items.push(Dependent { kind: "deck".into(), id: item.parent_id, name: format!("{} · {}", item.name, item.parent_name) });
        }
        Ok(items)
    }

    fn attach_audit_relations(&self, entries: &mut [AuditRow]) -> AppResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let placeholders = (1..=entries.len()).map(|index| format!("?{index}")).collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT ref_id, relation FROM query_relations WHERE kind = 'audit' AND ref_id IN ({placeholders}) ORDER BY relation"
        );
        let ids: Vec<String> = entries.iter().map(|entry| entry.id.to_string()).collect();
        let refs: Vec<&dyn ToSql> = ids.iter().map(|id| id as &dyn ToSql).collect();
        let mut statement = self.conn.prepare(&sql)?;
        let mut by_id: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        let rows = statement.query_map(refs.as_slice(), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for row in rows {
            let (ref_id, relation) = row?;
            by_id.entry(ref_id).or_default().push(relation);
        }
        for entry in entries {
            entry.relations = by_id.remove(&entry.id.to_string()).unwrap_or_default();
        }
        Ok(())
    }

    pub fn list_audit(&self, filter: &AuditFilter) -> AppResult<AuditPage> {
        let mut entries = self.query_audit(filter, Some(AUDIT_PAGE + 1))?;
        let has_more = entries.len() as i64 > AUDIT_PAGE;
        if has_more {
            entries.truncate(AUDIT_PAGE as usize);
        }
        Ok(AuditPage { entries, has_more })
    }

    pub fn export_audit(&self, filter: &AuditFilter) -> AppResult<Vec<AuditRow>> {
        self.query_audit(filter, None)
    }

    fn query_audit(&self, filter: &AuditFilter, limit: Option<i64>) -> AppResult<Vec<AuditRow>> {
        if let Some(status) = filter.status.as_deref() {
            if !matches!(status, "ok" | "error" | "denied") {
                return Err(AppError::invalid("Status is ok, error or denied"));
            }
        }
        let kind = filter.kind.clone().unwrap_or_default();
        let status = filter.status.clone().unwrap_or_default();
        let search = filter.search.as_deref().map(like_contains).unwrap_or_default();
        let before_ts = filter.before_ts.clone().unwrap_or_default();
        let before_id = filter.before_id.unwrap_or(0);
        let mut sql = String::from(
            "SELECT id, ts, actor, kind, question, sql, rows, duration_ms, status, detail_json
             FROM audit_log
             WHERE (?1 = '' OR kind = ?1)
               AND (?2 = '' OR status = ?2)
               AND (?3 = '' OR IFNULL(question, '') LIKE ?3 ESCAPE '\\' OR IFNULL(sql, '') LIKE ?3 ESCAPE '\\')
               AND (?4 = '' OR ts < ?4 OR (ts = ?4 AND id < ?5))
             ORDER BY ts DESC, id DESC",
        );
        let mut bound: Vec<Box<dyn ToSql>> = vec![
            Box::new(kind),
            Box::new(status),
            Box::new(search),
            Box::new(before_ts),
            Box::new(before_id),
        ];
        if let Some(limit) = limit {
            sql.push_str(" LIMIT ?6");
            bound.push(Box::new(limit));
        }
        let mut statement = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn ToSql> = bound.iter().map(|value| value.as_ref()).collect();
        let rows = statement.query_map(refs.as_slice(), map_audit)?;
        let mut entries: Vec<AuditRow> = rows.collect::<Result<_, _>>()?;
        self.attach_audit_relations(&mut entries)?;
        Ok(entries)
    }
}

type RawConnection = (String, String, String, String, Option<String>, String);

fn raw_connection(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawConnection> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
}

fn decode_connection((id, kind, name, config, secret_ref, created_at): RawConnection) -> AppResult<ConnectionRecord> {
    Ok(ConnectionRecord {
        id,
        kind: ConnectorKind::parse(&kind)?,
        name,
        config: serde_json::from_str(&config)?,
        secret_ref,
        created_at,
    })
}

fn map_table(row: &rusqlite::Row<'_>) -> rusqlite::Result<TableRecord> {
    Ok(TableRecord {
        id: row.get(0)?,
        connection_id: row.get(1)?,
        source_ref: row.get(2)?,
        relation: row.get(3)?,
        description: row.get(4)?,
        row_count: row.get(5)?,
        profile_error: row.get(6)?,
        profiled_at: row.get(7)?,
    })
}

fn map_audit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditRow> {
    let detail: Option<String> = row.get(9)?;
    Ok(AuditRow {
        id: row.get(0)?,
        ts: row.get(1)?,
        actor: row.get(2)?,
        kind: row.get(3)?,
        question: row.get(4)?,
        sql: row.get(5)?,
        rows: row.get(6)?,
        duration_ms: row.get(7)?,
        status: row.get(8)?,
        detail: detail.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null),
        relations: Vec::new(),
    })
}

fn like_contains(raw: &str) -> String {
    let escaped = raw.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
}

impl WorkspaceDb {
    pub fn search_workspace(&self, query: &str) -> AppResult<Vec<SearchHit>> {
        let needle = query.trim();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = like_contains(needle);
        let sql = "
            SELECT 'chat' AS kind, id, title, NULL FROM conversations WHERE title LIKE ?1 ESCAPE '\\'
            UNION ALL
            SELECT 'dashboard', id, name, NULL FROM dashboards WHERE name LIKE ?1 ESCAPE '\\'
            UNION ALL
            SELECT 'datasite', id, name, NULL FROM datasites WHERE name LIKE ?1 ESCAPE '\\'
            UNION ALL
            SELECT 'automation', id, name, NULL FROM automations WHERE name LIKE ?1 ESCAPE '\\'
            UNION ALL
            SELECT 'query', id, name, sql FROM saved_queries WHERE name LIKE ?1 ESCAPE '\\' OR sql LIKE ?1 ESCAPE '\\'
            LIMIT 40
        ";
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map([&pattern], |row| {
            Ok(SearchHit { kind: row.get(0)?, id: row.get(1)?, title: row.get(2)?, subtitle: row.get(3)? })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }
}
