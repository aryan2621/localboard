pub mod theme;
pub mod artifacts;
pub mod agents;
pub mod audit;
pub mod bench;
pub mod automations;
pub mod dashboards;
pub mod analyst;
pub mod catalog;
pub mod connections;
pub mod llm;
pub mod system;
pub mod workspaces;
pub mod mcp;
pub mod identity;
pub mod backup;
pub mod deeplink;

pub const ANALYST_EVENT: &str = "analyst://event";
pub const CATALOG_CHANGED: &str = "catalog://changed";
pub const DOWNLOAD_PROGRESS: &str = "llm://download-progress";
pub const APP_READY: &str = "app://ready";

pub const GENERATE_PROGRESS: &str = "generate://progress";

/// Reports one stage of building a dashboard, datasite, report, deck or automation. A no-op without a window to tell.
pub fn progress(
    app: Option<&tauri::AppHandle>,
    id: &str,
    label: impl Into<String>,
    status: crate::engine::analyst::StepStatus,
    detail: Option<String>,
) {
    let Some(app) = app else { return };
    let step = crate::engine::analyst::Step { id: id.to_owned(), label: label.into(), status, detail };
    if let Err(error) = tauri::Emitter::emit(app, GENERATE_PROGRESS, step) {
        tracing::warn!("could not deliver generation progress: {error}");
    }
}
