//! The workspace brand (name and colour) used on datasites, reports and decks.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::workspace_db::WorkspaceDb;
use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTheme {
    pub brand: String,
    pub name: String,
}

impl WorkspaceDb {
    pub fn theme(&self) -> AppResult<WorkspaceTheme> {
        self.conn
            .query_row("SELECT brand, name FROM workspace_theme WHERE id = 1", [], |row| {
                Ok(WorkspaceTheme { brand: row.get(0)?, name: row.get(1)? })
            })
            .optional()?
            .ok_or_else(|| AppError::not_found("Theme is missing"))
    }

    pub fn set_theme(&self, brand: &str, name: &str) -> AppResult<WorkspaceTheme> {
        let brand = brand.trim();
        let name = name.trim();
        if !brand.starts_with('#') || brand.len() != 7 {
            return Err(AppError::invalid("Brand colour is a #RRGGBB value"));
        }
        if name.is_empty() {
            return Err(AppError::invalid("Name the brand"));
        }
        self.conn.execute("UPDATE workspace_theme SET brand = ?1, name = ?2 WHERE id = 1", params![brand, name])?;
        self.theme()
    }
}
