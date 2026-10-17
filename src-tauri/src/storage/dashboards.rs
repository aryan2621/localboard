//! Dashboards, tiles, folders and version snapshots.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::workspace_db::WorkspaceDb;
use crate::engine::analyst::chart::ChartSpec;
use crate::error::{AppError, AppResult};

fn now() -> &'static str {
    "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileLayout {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilterDef {
    pub name: String,
    pub label: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRecord {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    pub id: String,
    pub name: String,
    pub folder_id: Option<String>,
    pub folder_name: Option<String>,
    pub tile_count: i64,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileRecord {
    pub id: String,
    pub dashboard_id: String,
    pub title: String,
    pub sql: String,
    pub chart: ChartSpec,
    pub layout: TileLayout,
    pub insight: Option<String>,
    pub facts_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardRecord {
    pub id: String,
    pub name: String,
    pub folder_id: Option<String>,
    pub prompt: String,
    pub filters: Vec<FilterDef>,
    pub refresh_secs: Option<i64>,
    pub tiles: Vec<TileRecord>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardVersion {
    pub id: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    name: String,
    folder_id: Option<String>,
    filters: Vec<FilterDef>,
    refresh_secs: Option<i64>,
    tiles: Vec<TileRecord>,
}

impl WorkspaceDb {
    pub fn list_folders(&self) -> AppResult<Vec<FolderRecord>> {
        let mut statement = self.conn.prepare("SELECT id, name FROM dashboard_folders ORDER BY name")?;
        let rows = statement.query_map([], |row| Ok(FolderRecord { id: row.get(0)?, name: row.get(1)? }))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn insert_folder(&self, id: &str, name: &str) -> AppResult<FolderRecord> {
        self.conn.execute("INSERT INTO dashboard_folders(id, name) VALUES (?1, ?2)", params![id, name])?;
        Ok(FolderRecord { id: id.to_owned(), name: name.to_owned() })
    }

    pub fn list_dashboards(&self) -> AppResult<Vec<DashboardSummary>> {
        let mut statement = self.conn.prepare(
            "SELECT d.id, d.name, d.folder_id, f.name, (SELECT COUNT(*) FROM dashboard_tiles t WHERE t.dashboard_id = d.id), d.updated_at
             FROM dashboards d LEFT JOIN dashboard_folders f ON f.id = d.folder_id ORDER BY d.updated_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DashboardSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                folder_id: row.get(2)?,
                folder_name: row.get(3)?,
                tile_count: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn dashboard(&self, id: &str) -> AppResult<DashboardRecord> {
        let (name, folder_id, prompt, filters, refresh_secs): (String, Option<String>, String, String, Option<i64>) = self
            .conn
            .query_row(
                "SELECT name, folder_id, prompt, filters_json, refresh_secs FROM dashboards WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Dashboard not found"))?;
        Ok(DashboardRecord { id: id.to_owned(), name, folder_id, prompt, filters: serde_json::from_str(&filters)?, refresh_secs, tiles: self.tiles(id)? })
    }

    pub fn insert_dashboard(
        &self,
        id: &str,
        name: &str,
        folder_id: Option<&str>,
        prompt: &str,
        filters: &[FilterDef],
        refresh_secs: Option<i64>,
        tiles: &[TileRecord],
    ) -> AppResult<DashboardRecord> {
        self.conn.execute(
            "INSERT INTO dashboards(id, folder_id, name, prompt, filters_json, refresh_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, folder_id, name, prompt, serde_json::to_string(filters)?, refresh_secs],
        )?;
        for tile in tiles {
            self.insert_tile(tile)?;
        }
        self.snapshot(id)?;
        self.dashboard(id)
    }

    pub fn insert_tile(&self, tile: &TileRecord) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO dashboard_tiles(id, dashboard_id, title, sql, chart_json, layout_json, insight, facts_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                tile.id,
                tile.dashboard_id,
                tile.title,
                tile.sql,
                serde_json::to_string(&tile.chart)?,
                serde_json::to_string(&tile.layout)?,
                tile.insight,
                tile.facts_hash
            ],
        )?;
        self.touch(&tile.dashboard_id)?;
        Ok(())
    }

    pub fn set_tile_text(&self, id: &str, title: &str, markdown: &str) -> AppResult<String> {
        let dashboard_id: Option<String> = self
            .conn
            .query_row("SELECT dashboard_id FROM dashboard_tiles WHERE id = ?1 AND chart_json LIKE '%\"text\"%'", [id], |row| row.get(0))
            .optional()?;
        let dashboard_id = dashboard_id.ok_or_else(|| AppError::invalid("That is not a text tile"))?;
        self.conn.execute("UPDATE dashboard_tiles SET title = ?1, sql = ?2 WHERE id = ?3", params![title, markdown, id])?;
        self.touch(&dashboard_id)?;
        Ok(dashboard_id)
    }

    /// Replaces a query tile's name, SQL and chart; returns its dashboard id.
    pub fn set_tile_query(&self, id: &str, title: &str, sql: &str, chart: &ChartSpec) -> AppResult<String> {
        let dashboard_id: Option<String> =
            self.conn.query_row("SELECT dashboard_id FROM dashboard_tiles WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        let dashboard_id = dashboard_id.ok_or_else(|| AppError::not_found("Tile not found"))?;
        self.conn.execute(
            "UPDATE dashboard_tiles SET title = ?1, sql = ?2, chart_json = ?3, insight = NULL, facts_hash = NULL WHERE id = ?4",
            params![title, sql, serde_json::to_string(chart)?, id],
        )?;
        self.touch(&dashboard_id)?;
        Ok(dashboard_id)
    }

    pub fn delete_tile(&self, id: &str) -> AppResult<()> {
        let dashboard_id: Option<String> =
            self.conn.query_row("SELECT dashboard_id FROM dashboard_tiles WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        if self.conn.execute("DELETE FROM dashboard_tiles WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Tile not found"));
        }
        if let Some(dashboard_id) = dashboard_id {
            self.touch(&dashboard_id)?;
        }
        Ok(())
    }

    pub fn update_layouts(&self, tiles: &[(String, TileLayout)]) -> AppResult<()> {
        for (id, layout) in tiles {
            self.conn.execute("UPDATE dashboard_tiles SET layout_json = ?1 WHERE id = ?2", params![serde_json::to_string(layout)?, id])?;
        }
        if let Some(id) = tiles.first().map(|(id, _)| id.clone()) {
            if let Ok(dashboard_id) = self.conn.query_row("SELECT dashboard_id FROM dashboard_tiles WHERE id = ?1", [id], |row| row.get::<_, String>(0)) {
                self.snapshot(&dashboard_id)?;
            }
        }
        Ok(())
    }

    pub fn update_dashboard(&self, id: &str, name: &str, folder_id: Option<&str>, filters: &[FilterDef], refresh_secs: Option<i64>) -> AppResult<DashboardRecord> {
        let changed = self.conn.execute(
            &format!(
                "UPDATE dashboards SET name = ?1, folder_id = ?2, filters_json = ?3, refresh_secs = ?4, updated_at = {now} WHERE id = ?5",
                now = now()
            ),
            params![name, folder_id, serde_json::to_string(filters)?, refresh_secs, id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Dashboard not found"));
        }
        self.snapshot(id)?;
        self.dashboard(id)
    }

    pub fn set_last_params(&self, id: &str, params_json: &str) -> AppResult<()> {
        self.conn.execute("UPDATE dashboards SET last_params_json = ?1 WHERE id = ?2", params![params_json, id])?;
        Ok(())
    }

    pub fn scheduled_dashboards(&self) -> AppResult<Vec<(String, i64)>> {
        let mut statement = self.conn.prepare("SELECT id, refresh_secs FROM dashboards WHERE refresh_secs IS NOT NULL AND refresh_secs > 0")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn last_params(&self, id: &str) -> AppResult<String> {
        self.conn
            .query_row("SELECT last_params_json FROM dashboards WHERE id = ?1", [id], |row| row.get::<_, String>(0))
            .optional()?
            .ok_or_else(|| AppError::not_found("Dashboard not found"))
    }

    pub fn set_tile_insight(&self, id: &str, insight: &str, facts_hash: &str) -> AppResult<()> {
        self.conn.execute("UPDATE dashboard_tiles SET insight = ?1, facts_hash = ?2 WHERE id = ?3", params![insight, facts_hash, id])?;
        Ok(())
    }

    pub fn delete_dashboard(&self, id: &str) -> AppResult<()> {
        let tiles: Vec<String> = {
            let mut statement = self.conn.prepare("SELECT id FROM dashboard_tiles WHERE dashboard_id = ?1")?;
            let rows = statement.query_map([id], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for tile in tiles {
            self.clear_query_relations("tile", &tile)?;
        }
        if self.conn.execute("DELETE FROM dashboards WHERE id = ?1", [id])? == 0 {
            return Err(AppError::not_found("Dashboard not found"));
        }
        Ok(())
    }

    pub fn versions(&self, dashboard_id: &str) -> AppResult<Vec<DashboardVersion>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, created_at FROM dashboard_versions WHERE dashboard_id = ?1 ORDER BY created_at DESC LIMIT 20")?;
        let rows = statement.query_map([dashboard_id], |row| Ok(DashboardVersion { id: row.get(0)?, created_at: row.get(1)? }))?;
        rows.collect::<Result<_, _>>().map_err(AppError::from)
    }

    pub fn restore_dashboard(&self, dashboard_id: &str, version_id: &str) -> AppResult<DashboardRecord> {
        let json: String = self
            .conn
            .query_row(
                "SELECT snapshot_json FROM dashboard_versions WHERE id = ?1 AND dashboard_id = ?2",
                params![version_id, dashboard_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("That version no longer exists"))?;
        let snapshot: Snapshot = serde_json::from_str(&json)?;
        self.conn.execute("DELETE FROM dashboard_tiles WHERE dashboard_id = ?1", [dashboard_id])?;
        self.conn.execute(
            &format!(
                "UPDATE dashboards SET name = ?1, folder_id = ?2, filters_json = ?3, refresh_secs = ?4, updated_at = {now} WHERE id = ?5",
                now = now()
            ),
            params![snapshot.name, snapshot.folder_id, serde_json::to_string(&snapshot.filters)?, snapshot.refresh_secs, dashboard_id],
        )?;
        for tile in snapshot.tiles {
            self.insert_tile(&tile)?;
        }
        self.dashboard(dashboard_id)
    }

    pub fn snapshot(&self, dashboard_id: &str) -> AppResult<()> {
        let dashboard = self.dashboard(dashboard_id)?;
        let json = serde_json::to_string(&Snapshot {
            name: dashboard.name,
            folder_id: dashboard.folder_id,
            filters: dashboard.filters,
            refresh_secs: dashboard.refresh_secs,
            tiles: dashboard.tiles,
        })?;
        self.conn.execute(
            "INSERT INTO dashboard_versions(id, dashboard_id, snapshot_json) VALUES (?1, ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), dashboard_id, json],
        )?;
        Ok(())
    }

    fn tiles(&self, dashboard_id: &str) -> AppResult<Vec<TileRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT id, dashboard_id, title, sql, chart_json, layout_json, insight, facts_hash FROM dashboard_tiles WHERE dashboard_id = ?1",
        )?;
        let rows = statement.query_map([dashboard_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (id, dashboard_id, title, sql, chart, layout, insight, facts_hash) = row?;
            Ok(TileRecord {
                id,
                dashboard_id,
                title,
                sql,
                chart: serde_json::from_str(&chart)?,
                layout: serde_json::from_str(&layout)?,
                insight,
                facts_hash,
            })
        })
        .collect()
    }

    fn touch(&self, id: &str) -> AppResult<()> {
        self.conn.execute(&format!("UPDATE dashboards SET updated_at = {now} WHERE id = ?1", now = now()), [id])?;
        Ok(())
    }
}
