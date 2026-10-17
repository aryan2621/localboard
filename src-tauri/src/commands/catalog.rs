use serde::Serialize;
use tauri::State;

use crate::engine::catalog;
use crate::engine::profile::SampleRow;
use crate::error::{blocking, AppError, AppResult};
use crate::state::AppState;
use crate::storage::workspace_db::{ColumnRecord, Dependent, TableRecord, TableUsage};

const MAX_DESCRIPTION_CHARS: usize = 2_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipView {
    /// `outgoing`: this table's column references another table; `incoming`: the reverse.
    pub direction: &'static str,
    pub column: String,
    pub other_table_id: String,
    pub other_relation: String,
    pub other_column: String,
    pub confidence: f64,
    pub origin: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDetail {
    pub table: TableRecord,
    pub columns: Vec<ColumnRecord>,
    pub samples: Vec<SampleRow>,
    pub relationships: Vec<RelationshipView>,
    pub used_by: TableUsage,
}

fn validate_description(description: &str) -> AppResult<()> {
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(AppError::invalid(format!("Descriptions are limited to {MAX_DESCRIPTION_CHARS} characters")));
    }
    Ok(())
}

#[tauri::command]
pub async fn list_tables(state: State<'_, AppState>) -> AppResult<Vec<TableRecord>> {
    let workspace = state.workspace()?;
    blocking(move || {
        workspace.db()?.list_tables()
    })
    .await
}

#[tauri::command]
pub async fn get_table(state: State<'_, AppState>, id: String) -> AppResult<TableDetail> {
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        let table = db.get_table(&id)?;
        let relations: std::collections::HashMap<String, String> =
            db.list_tables()?.into_iter().map(|other| (other.id, other.relation)).collect();
        let relationships = db
            .relationships()?
            .into_iter()
            .filter_map(|relationship| {
                let (direction, column, other_id, other_column) = if relationship.from_table_id == id {
                    ("outgoing", relationship.from_column, relationship.to_table_id, relationship.to_column)
                } else if relationship.to_table_id == id {
                    ("incoming", relationship.to_column, relationship.from_table_id, relationship.from_column)
                } else {
                    return None;
                };
                Some(RelationshipView {
                    direction,
                    column,
                    other_relation: relations.get(&other_id).cloned().unwrap_or_default(),
                    other_table_id: other_id,
                    other_column,
                    confidence: relationship.confidence,
                    origin: relationship.origin,
                })
            })
            .collect();
        let used_by = db.table_usage(&table.relation)?;
        Ok(TableDetail { columns: db.columns(&id)?, samples: db.samples(&id)?, relationships, used_by, table })
    })
    .await
}

#[tauri::command]
pub async fn connection_dependents(state: State<'_, AppState>, id: String) -> AppResult<Vec<Dependent>> {
    let workspace = state.workspace()?;
    blocking(move || {
        let db = workspace.db()?;
        let relations = db.tables_for_connection(&id)?.into_iter().map(|table| table.relation).collect::<Vec<_>>();
        db.dependents_of_relations(&relations)
    })
    .await
}

#[tauri::command]
pub async fn rename_table(state: State<'_, AppState>, id: String, name: String) -> AppResult<TableRecord> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || catalog::rename_table(&workspace, &id, &name)).await
}

#[tauri::command]
pub async fn describe_table(state: State<'_, AppState>, id: String, description: String) -> AppResult<()> {
    validate_description(&description)?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.set_table_description(&id, &description)).await
}

#[tauri::command]
pub async fn describe_column(state: State<'_, AppState>, table_id: String, column: String, description: String) -> AppResult<()> {
    validate_description(&description)?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.set_column_description(&table_id, &column, &description)).await
}
