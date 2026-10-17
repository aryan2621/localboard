mod commands;
mod connectors;
mod engine;
mod error;
mod llm;
mod notifier;
mod retrieval;
mod server;
mod sql_guard;
mod state;
mod storage;

use std::path::Path;
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use crate::llm::llama_server::Supervisor;
use crate::llm::LlmRuntime;
use crate::state::AppState;
use crate::storage::app_db::AppDb;

/// Logs stay on this machine, in the app data folder.
fn init_logging(data_dir: &Path) {
    let logs = data_dir.join("logs");
    let file = std::fs::create_dir_all(&logs)
        .and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(logs.join("localboard.log")));
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    match file {
        Ok(file) => {
            let _ = tracing_subscriber::fmt().with_writer(Mutex::new(file)).with_ansi(false).with_env_filter(filter).try_init();
        }
        Err(error) => eprintln!("localboard: logging to stderr, log file unavailable: {error}"),
    }
}

pub fn mcp_stdio() {
    crate::engine::mcp::stdio_proxy();
}

/// Marks the app ready on every exit path, including a panic during workspace open.
struct BootReady {
    handle: tauri::AppHandle,
}

impl Drop for BootReady {
    fn drop(&mut self) {
        if let Some(state) = self.handle.try_state::<AppState>() {
            state.mark_ready();
        }
        let _ = self.handle.emit(commands::APP_READY, ());
    }
}

fn finish_boot(handle: tauri::AppHandle) {
    let _ready = BootReady { handle: handle.clone() };
    let state = handle.state::<AppState>();
    state.llm.stop_leftover_servers();
    if let Err(error) = commands::workspaces::open_initial(&state) {
        tracing::error!("could not open a workspace at startup: {error}");
    }
    commands::automations::resume(&handle);
    let _ = commands::mcp::APP_HANDLE.set(handle.clone());
    match crate::server::start(handle.clone()) {
        Ok(server) => state.set_server(server),
        Err(error) => {
            tracing::error!("local site server: {error}");
            state.fail_server(error.to_string());
        }
    }
    drop(state);
    notifier::setup();
    tracing::info!("startup finished");
    drop(_ready);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            init_logging(&data_dir);

            let app_db = AppDb::open(&data_dir.join("app.sqlite"))?;
            let binary = Supervisor::locate_binary();
            if binary.is_none() {
                tracing::warn!("llama-server not found; AI features are unavailable (SQL-only mode)");
            }
            let llm = Arc::new(LlmRuntime::new(data_dir.clone(), binary)?);
            let handle = app.handle().clone();
            let notifier: state::ChangeNotifier = Arc::new(move |workspace_id: &str| {
                let _ = handle.emit(commands::CATALOG_CHANGED, workspace_id);
            });
            app.manage(AppState::new(data_dir, app_db, llm, notifier));

            commands::identity::restore_lock_and_offline(&app.state::<AppState>());
            commands::deeplink::register_deep_links(app.handle());
            if let Err(error) = commands::automations::install_tray(app.handle()) {
                tracing::warn!("tray icon unavailable: {error}");
            }

            // Workspace open, leftover llama-server cleanup and the loopback server stay off this
            // thread. Setup runs on the macOS main thread; blocking it here leaves the webview
            // painted on the HTML splash with JavaScript never started.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let joined = tauri::async_runtime::spawn_blocking(move || finish_boot(handle)).await;
                if let Err(error) = joined {
                    tracing::error!("startup task failed: {error}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_status,
            commands::system::app_run_in_background,
            commands::system::app_force_quit,
            commands::system::notify_test,
            commands::system::notify_open_settings,
            commands::system::complete_onboarding,
            commands::system::search_workspace,
            commands::system::local_mcp_info,
            commands::system::local_mcp_rotate_token,
            commands::workspaces::list_workspaces,
            commands::workspaces::create_workspace,
            commands::workspaces::switch_workspace,
            commands::workspaces::rename_workspace,
            commands::workspaces::delete_workspace,
            commands::connections::list_connections,
            commands::connections::add_file_source,
            commands::connections::refresh_connection,
            commands::connections::remove_connection,
            commands::catalog::connection_dependents,
            commands::connections::test_db_connection,
            commands::connections::save_db_connection,
            commands::connections::explore_db_connection,
            commands::connections::register_db_tables,
            commands::catalog::list_tables,
            commands::catalog::get_table,
            commands::catalog::rename_table,
            commands::catalog::describe_table,
            commands::catalog::describe_column,
            commands::llm::llm_catalog,
            commands::llm::llm_download,
            commands::llm::llm_cancel_download,
            commands::llm::llm_delete_model,
            commands::llm::llm_benchmark,
            commands::llm::llm_stop,
            commands::llm::set_task_routing,
            commands::analyst::ask,
            commands::analyst::cancel_answer,
            commands::analyst::list_conversations,
            commands::analyst::get_conversation,
            commands::analyst::delete_conversation,
            commands::analyst::rename_conversation,
            commands::analyst::export_conversation,
            commands::analyst::rerun_query,
            commands::analyst::set_feedback,
            commands::analyst::starter_questions,
            commands::analyst::completion_sources,
            commands::agents::list_agents,
            commands::agents::create_agent,
            commands::agents::update_agent,
            commands::agents::delete_agent,
            commands::agents::list_rules,
            commands::agents::save_rule,
            commands::agents::delete_rule,
            commands::agents::list_rule_versions,
            commands::agents::rollback_rule,
            commands::agents::list_examples,
            commands::agents::save_example,
            commands::agents::delete_example,
            commands::agents::save_answer_example,
            commands::agents::list_memories,
            commands::agents::save_memory,
            commands::agents::set_memory_status,
            commands::agents::delete_memory,
            commands::agents::list_knowledge,
            commands::agents::add_knowledge,
            commands::agents::delete_knowledge,
            commands::bench::bench_schema,
            commands::bench::bench_format,
            commands::bench::bench_run,
            commands::bench::bench_explain,
            commands::bench::bench_fix,
            commands::bench::bench_optimize,
            commands::bench::bench_describe,
            commands::bench::bench_export,
            commands::bench::bench_history,
            commands::bench::bench_saved,
            commands::bench::bench_save_query,
            commands::bench::bench_delete_query,
            commands::dashboards::dashboard_folders,
            commands::dashboards::dashboard_create_folder,
            commands::dashboards::dashboard_list,
            commands::dashboards::dashboard_get,
            commands::dashboards::dashboard_generate,
            commands::dashboards::dashboard_add_tile,
            commands::dashboards::dashboard_delete_tile,
            commands::dashboards::dashboard_add_text_tile,
            commands::dashboards::dashboard_update_text_tile,
            commands::dashboards::dashboard_update_tile,
            commands::dashboards::dashboard_save_layout,
            commands::dashboards::dashboard_update,
            commands::dashboards::dashboard_delete,
            commands::dashboards::dashboard_duplicate,
            commands::dashboards::dashboard_versions,
            commands::dashboards::dashboard_restore,
            commands::dashboards::dashboard_refresh,
            commands::automations::automation_list,
            commands::automations::automation_save,
            commands::automations::automation_delete,
            commands::automations::automation_runs,
            commands::automations::automation_run,
            commands::automations::automation_confirm,
            commands::automations::automation_rotate_webhook,
            commands::automations::automation_propose,
            commands::automations::automation_set_step_secret,
            commands::automations::smtp_get,
            commands::automations::smtp_save,
            commands::audit::audit_list,
            commands::audit::audit_export,
            commands::artifacts::datasite_list,
            commands::artifacts::datasite_get,
            commands::artifacts::datasite_generate,
            commands::artifacts::datasite_from_answer,
            commands::artifacts::datasite_delete,
            commands::artifacts::datasite_export,
            commands::artifacts::report_list,
            commands::artifacts::report_get,
            commands::artifacts::report_generate,
            commands::artifacts::report_from_answer,
            commands::artifacts::report_delete,
            commands::artifacts::report_export_pdf,
            commands::artifacts::deck_list,
            commands::artifacts::deck_get,
            commands::artifacts::deck_generate,
            commands::artifacts::deck_from_answer,
            commands::artifacts::deck_update_outline,
            commands::artifacts::deck_delete,
            commands::artifacts::deck_export_pptx,
            commands::theme::app_theme,
            commands::theme::app_set_theme,
            commands::mcp::mcp_list_servers,
            commands::mcp::mcp_save_server,
            commands::mcp::mcp_delete_server,
            commands::mcp::mcp_list_tools,
            commands::mcp::mcp_list_rules,
            commands::mcp::mcp_set_rule,
            commands::mcp::mcp_delete_rule,
            commands::identity::governance_status,
            commands::identity::set_offline,
            commands::identity::set_app_lock,
            commands::identity::unlock_app,
            commands::identity::lock_app,
            commands::identity::unlock_workspace,
            commands::identity::mask_list,
            commands::identity::mask_save,
            commands::identity::mask_delete,
            commands::identity::encrypt_workspace,
            commands::backup::backup_export,
            commands::backup::backup_import,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build LocalBoard");

    app.run(|handle, event| {
        match event {
            RunEvent::WindowEvent { event: WindowEvent::CloseRequested { api, .. }, .. } => {
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.hide();
                    api.prevent_close();
                    // Hidden, LocalBoard keeps running its automations but leaves the menu bar and Dock.
                    #[cfg(target_os = "macos")]
                    let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
            // Clicking the Dock icon after the window was hidden brings it back instead of leaving nothing on screen.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            RunEvent::Exit => {
                if let Some(state) = handle.try_state::<AppState>() {
                    state.shutdown_server();
                    state.llm.shutdown();
                }
            }
            _ => {}
        }
    });
}
