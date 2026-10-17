use serde::Serialize;
use tauri::State;

use crate::error::{blocking, AppResult};
use crate::llm::hardware::Hardware;
use crate::state::AppState;
use crate::storage::workspace_db::SearchHit;

const ONBOARDING_KEY: &str = "onboarding.completed";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServerInfo {
    pub port: u16,
    pub url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub version: &'static str,
    pub onboarding_completed: bool,
    pub hardware: Hardware,
    pub local_server: Option<LocalServerInfo>,
    pub local_server_error: Option<String>,
    pub offline: bool,
    pub locked: bool,
    pub lock_enabled: bool,
    /// False while workspace open is still running off the UI thread.
    pub ready: bool,
    /// Set when the newest workspace is encrypted and its passphrase is not remembered: call unlock_workspace.
    pub needs_passphrase: Option<String>,
}

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    // One lock for the whole read. A second app_db() in this same statement would deadlock:
    // the first guard stays alive until the statement ends, and this command runs on the UI thread.
    let db = state.app_db()?;
    let onboarding_completed = db.setting::<bool>(ONBOARDING_KEY)?.unwrap_or(false);
    let lock_enabled = db.setting::<bool>("lock.enabled")?.unwrap_or(false);
    drop(db);
    let needs_passphrase = if state.active_id()?.is_none() {
        let db = state.app_db()?;
        db.most_recent_workspace()?.filter(|workspace| workspace.encrypted).map(|workspace| workspace.id)
    } else {
        None
    };
    Ok(AppStatus {
        version: env!("CARGO_PKG_VERSION"),
        onboarding_completed,
        hardware: state.llm.hardware().clone(),
        local_server: state.server_origin().map(|url| LocalServerInfo {
            port: url.rsplit(':').next().and_then(|port| port.parse().ok()).unwrap_or(0),
            url,
        }),
        local_server_error: state.server_error(),
        offline: state.is_offline(),
        locked: lock_enabled && !state.is_unlocked(),
        lock_enabled,
        ready: state.is_ready(),
        needs_passphrase,
    })
}

#[tauri::command]
pub fn complete_onboarding(app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    state.app_db()?.set_setting(ONBOARDING_KEY, &true)?;
    // The first notification is what registers the app in System Settings → Notifications.
    let _ = show_notification(&app, "LocalBoard", "Notifications are on. Automations can alert you here.");
    Ok(())
}

fn show_notification(app: &tauri::AppHandle, title: &str, body: &str) -> AppResult<()> {
    crate::notifier::show(app, title, body).map_err(|error| crate::error::AppError::invalid(format!("Could not show a notification: {error}")))
}

#[tauri::command]
pub fn notify_test(app: tauri::AppHandle) -> AppResult<()> {
    show_notification(&app, "LocalBoard", "Test notification. If you can read this, alerts will reach you.")
}

#[tauri::command]
pub fn notify_open_settings() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension").spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "ms-settings:notifications"]).spawn();
}

#[tauri::command]
pub async fn search_workspace(state: State<'_, AppState>, query: String) -> AppResult<Vec<SearchHit>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.search_workspace(&query)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMcpInfo {
    pub url: String,
    pub token: String,
    /// What to put in Claude Desktop or Cursor to reach LocalBoard.
    pub proxy_command: String,
}

fn mcp_info(token: String) -> LocalMcpInfo {
    LocalMcpInfo {
        url: format!("http://127.0.0.1:{}/mcp", crate::commands::artifacts::LOCAL_PORT),
        token,
        proxy_command: "localboard mcp".into(),
    }
}

/// The bearer token for LocalBoard's local MCP endpoint, for Settings to show and copy.
#[tauri::command]
pub fn local_mcp_info(state: State<'_, AppState>) -> AppResult<LocalMcpInfo> {
    state.ensure_unlocked()?;
    Ok(mcp_info(crate::engine::mcp::local_token()?))
}

#[tauri::command]
pub fn local_mcp_rotate_token(state: State<'_, AppState>) -> AppResult<LocalMcpInfo> {
    state.ensure_unlocked()?;
    Ok(mcp_info(crate::engine::mcp::rotate_local_token()?))
}

/// Quit: hide the window and leave LocalBoard running its automations in the background.
#[tauri::command]
pub fn app_run_in_background(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// Force quit: shut down everything, including the local AI server and the automation scheduler.
#[tauri::command]
pub fn app_force_quit(app: tauri::AppHandle) {
    app.exit(0);
}
