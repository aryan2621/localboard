use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::migrations;
use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_opened_at: Option<String>,
    pub encrypted: bool,
}

/// Global, workspace-independent state: the workspace registry and app settings.
pub struct AppDb {
    conn: Connection,
}

impl AppDb {
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        migrations::apply(&mut conn, migrations::APP)?;
        Ok(Self { conn })
    }

    pub fn list_workspaces(&self) -> AppResult<Vec<WorkspaceRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, created_at, last_opened_at, encrypted FROM workspaces ORDER BY created_at, name",
        )?;
        let rows = statement.query_map([], map_workspace)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get_workspace(&self, id: &str) -> AppResult<WorkspaceRecord> {
        self.conn
            .query_row(
                "SELECT id, name, created_at, last_opened_at, encrypted FROM workspaces WHERE id = ?1",
                [id],
                map_workspace,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Workspace not found"))
    }

    pub fn create_workspace(&self, id: &str, name: &str) -> AppResult<WorkspaceRecord> {
        self.conn
            .execute("INSERT INTO workspaces(id, name) VALUES (?1, ?2)", params![id, validate_name(name)?])?;
        self.get_workspace(id)
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> AppResult<WorkspaceRecord> {
        let changed = self
            .conn
            .execute("UPDATE workspaces SET name = ?1 WHERE id = ?2", params![validate_name(name)?, id])?;
        if changed == 0 {
            return Err(AppError::not_found("Workspace not found"));
        }
        self.get_workspace(id)
    }

    pub fn delete_workspace(&self, id: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn touch_workspace(&self, id: &str) -> AppResult<()> {
        self.conn.execute(
            "UPDATE workspaces SET last_opened_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    pub fn most_recent_workspace(&self) -> AppResult<Option<WorkspaceRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, created_at, last_opened_at, encrypted FROM workspaces
                 ORDER BY last_opened_at IS NULL, last_opened_at DESC, created_at LIMIT 1",
                [],
                map_workspace,
            )
            .optional()?)
    }

    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> AppResult<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value_json FROM app_settings WHERE key = ?1", [key], |row| row.get(0))
            .optional()?;
        raw.map(|json| serde_json::from_str(&json)).transpose().map_err(AppError::from)
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO app_settings(key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
}

fn validate_name(name: &str) -> AppResult<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 80 {
        return Err(AppError::invalid("Workspace names must be 1 to 80 characters"));
    }
    Ok(trimmed)
}

fn map_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRecord> {
    Ok(WorkspaceRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        last_opened_at: row.get(3)?,
        encrypted: row.get::<_, i64>(4)? == 1,
    })
}

impl AppDb {
    pub fn workspace_encrypted(&self, id: &str) -> AppResult<bool> {
        Ok(self.get_workspace(id)?.encrypted)
    }

    pub fn set_encrypted(&self, id: &str, encrypted: bool) -> AppResult<()> {
        let changed = self.conn.execute("UPDATE workspaces SET encrypted = ?1 WHERE id = ?2", params![i64::from(encrypted), id])?;
        if changed == 0 {
            return Err(AppError::not_found("Workspace not found"));
        }
        Ok(())
    }
}
