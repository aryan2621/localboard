use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use super::DOWNLOAD_PROGRESS;
use crate::error::AppResult;
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::llm::{Benchmark, CatalogView};
use crate::state::AppState;

fn routing(state: &AppState) -> AppResult<TaskRouting> {
    Ok(state.app_db()?.setting(SETTINGS_KEY)?.unwrap_or_default())
}

#[tauri::command]
pub async fn llm_catalog(state: State<'_, AppState>) -> AppResult<CatalogView> {
    let routing = routing(&state)?;
    Ok(state.llm.catalog(routing).await)
}

#[tauri::command]
pub fn llm_download(app: AppHandle, state: State<'_, AppState>, model_id: String) -> AppResult<()> {
    state.ensure_online("Downloading a model")?;
    state.llm.download(
        &model_id,
        Arc::new(move |progress| {
            let _ = app.emit(DOWNLOAD_PROGRESS, progress);
        }),
    )?;
    Ok(())
}

#[tauri::command]
pub fn llm_cancel_download(state: State<'_, AppState>) {
    state.llm.cancel_download();
}

#[tauri::command]
pub async fn llm_delete_model(state: State<'_, AppState>, model_id: String) -> AppResult<()> {
    Ok(state.llm.delete(&model_id).await?)
}

#[tauri::command]
pub async fn llm_benchmark(state: State<'_, AppState>, model_id: String) -> AppResult<Benchmark> {
    Ok(state.llm.benchmark(&model_id).await?)
}

#[tauri::command]
pub async fn llm_stop(state: State<'_, AppState>) -> AppResult<()> {
    state.llm.stop().await;
    Ok(())
}

/// Replaces the per-task model choices; every id is validated against the catalog.
#[tauri::command]
pub fn set_task_routing(state: State<'_, AppState>, routing: TaskRouting) -> AppResult<TaskRouting> {
    let mut validated = TaskRouting::default();
    validated.set(crate::llm::provider::Task::Sql, routing.sql)?;
    validated.set(crate::llm::provider::Task::Reasoning, routing.reasoning)?;
    validated.set(crate::llm::provider::Task::Narrative, routing.narrative)?;
    state.app_db()?.set_setting(SETTINGS_KEY, &validated)?;
    Ok(validated)
}
