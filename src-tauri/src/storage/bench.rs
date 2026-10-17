//! Query Bench history and saved queries.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::workspace_db::WorkspaceDb;
use crate::error::{AppError, AppResult};

fn now() -> &'static str {
    "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaTable {
    pub relation: String,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    pub id: String,
    pub name: String,
    pub sql: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub sql: String,
    pub params_json: String,
    pub status: String,
    pub error: Option<String>,
    pub row_count: Option<i64>,
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

impl WorkspaceDb {
    pub fn schema_index(&self) -> AppResult<Vec<SchemaTable>> {
        let tables = self.list_tables()?;
        let mut index = Vec::with_capacity(tables.len());
        for table in tables {
            let columns = self.columns(&table.id)?.into_iter().map(|column| column.name).collect();
            index.push(SchemaTable { relation: table.relation, columns });
        }
        Ok(index)
    }

    pub fn list_saved_queries(&self) -> AppResult<Vec<SavedQuery>> {
        let mut statement = self.conn.prepare("SELECT id, name, sql, updated_at FROM saved_queries ORDER BY updated_at DESC")?;
        let rows = statement.query_map([], |row| {
            Ok(SavedQuery { id: row.get(0)?, name: row.get(1)?, sql: row.get(2)?, updated_at: row.get(3)? })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn save_query(&self, id: &str, name: &str, sql: &str) -> AppResult<SavedQuery> {
        self.conn.execute(
            &format!(
                "INSERT INTO saved_queries(id, name, sql) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, sql = excluded.sql, updated_at = {now}",
                now = now()
            ),
            params![id, name, sql],
        )?;
        self.saved_query(id)
    }

    pub fn delete_saved_query(&self, id: &str) -> AppResult<()> {
        if self.conn.execute("DELETE FROM saved_queries WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Saved query not found"));
        }
        Ok(())
    }

    pub fn insert_history(
        &self,
        id: &str,
        sql: &str,
        params_json: &str,
        status: &str,
        error: Option<&str>,
        row_count: Option<i64>,
        duration_ms: Option<i64>,
    ) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO query_history(id, sql, params_json, status, error, row_count, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, sql, params_json, status, error, row_count, duration_ms],
        )?;
        Ok(())
    }

    pub fn list_history(&self) -> AppResult<Vec<HistoryEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT id, sql, params_json, status, error, row_count, duration_ms, created_at
             FROM query_history ORDER BY created_at DESC LIMIT 50",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                sql: row.get(1)?,
                params_json: row.get(2)?,
                status: row.get(3)?,
                error: row.get(4)?,
                row_count: row.get(5)?,
                duration_ms: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn saved_query(&self, id: &str) -> AppResult<SavedQuery> {
        self.conn
            .query_row("SELECT id, name, sql, updated_at FROM saved_queries WHERE id = ?1", [id], |row| {
                Ok(SavedQuery { id: row.get(0)?, name: row.get(1)?, sql: row.get(2)?, updated_at: row.get(3)? })
            })
            .optional()?
            .ok_or_else(|| AppError::not_found("Saved query not found"))
    }

}

