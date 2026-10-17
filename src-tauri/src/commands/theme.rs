//! Workspace brand commands.

use tauri::State;

use crate::error::{blocking, AppResult};
use crate::state::AppState;
use crate::storage::theme::WorkspaceTheme;

#[tauri::command]
pub async fn app_theme(state: State<'_, AppState>) -> AppResult<WorkspaceTheme> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.theme()).await
}

#[tauri::command]
pub async fn app_set_theme(state: State<'_, AppState>, brand: String, name: String) -> AppResult<WorkspaceTheme> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.set_theme(&brand, &name)).await
}
