use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::llm::provider::Task;
use crate::llm::router::{TaskRouting, SETTINGS_KEY};
use crate::llm::{LlmConfig, CONFIG_KEY};
use crate::state::AppState;
use crate::storage::secrets;

#[derive(Serialize)]
pub struct LlmConfigView {
    #[serde(flatten)]
    config: LlmConfig,
    has_key: bool,
    routing: TaskRouting,
}

#[derive(Serialize)]
pub struct LlmTest {
    ok: bool,
    message: String,
}

/// The key is never returned; the UI only learns whether one is saved.
#[tauri::command]
pub fn llm_get_config(state: State<'_, AppState>) -> AppResult<LlmConfigView> {
    state.ensure_unlocked()?;
    let config = state.llm_config()?;
    let has_key = secrets::get(&config.kind.secret_ref()).is_ok();
    Ok(LlmConfigView { config, has_key, routing: state.task_routing()? })
}

/// `api_key`: `None` keeps the saved key, `Some("")` clears it.
#[tauri::command]
pub fn llm_set_config(state: State<'_, AppState>, config: LlmConfig, api_key: Option<String>) -> AppResult<()> {
    state.ensure_unlocked()?;
    match api_key.as_deref().map(str::trim) {
        Some("") => secrets::delete(&config.kind.secret_ref())?,
        Some(key) => secrets::save(&config.kind.secret_ref(), key)?,
        None => {}
    }
    let config = LlmConfig {
        base_url: config.base_url.trim().to_owned(),
        model: config.model.trim().to_owned(),
        embed_model: config.embed_model.map(|model| model.trim().to_owned()).filter(|model| !model.is_empty()),
        ..config
    };
    state.app_db()?.set_setting(CONFIG_KEY, &config)?;
    Ok(())
}

#[tauri::command]
pub async fn llm_test(state: State<'_, AppState>) -> AppResult<LlmTest> {
    state.ensure_unlocked()?;
    let outcome: AppResult<()> = async { Ok(state.llm()?.provider(None)?.health().await?) }.await;
    Ok(match outcome {
        Ok(()) => LlmTest { ok: true, message: "Connected".into() },
        Err(error) => LlmTest { ok: false, message: error.to_string() },
    })
}

#[tauri::command]
pub fn set_task_routing(state: State<'_, AppState>, routing: TaskRouting) -> AppResult<TaskRouting> {
    state.ensure_unlocked()?;
    let mut clean = TaskRouting::default();
    clean.set(Task::Sql, routing.sql)?;
    clean.set(Task::Reasoning, routing.reasoning)?;
    clean.set(Task::Narrative, routing.narrative)?;
    state.app_db()?.set_setting(SETTINGS_KEY, &clean)?;
    Ok(clean)
}
