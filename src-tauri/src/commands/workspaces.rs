use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use rusqlite::Connection;

use crate::error::{blocking, AppError, AppResult};
use crate::state::{AppState, WORKSPACE_DB};
use crate::storage::app_db::WorkspaceRecord;
use crate::storage::{secrets, workspace_db::WorkspaceDb};

pub const DEFAULT_WORKSPACE_NAME: &str = "My workspace";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceList {
    pub workspaces: Vec<WorkspaceRecord>,
    pub active_id: Option<String>,
}

fn snapshot(state: &AppState) -> AppResult<WorkspaceList> {
    Ok(WorkspaceList { workspaces: state.app_db()?.list_workspaces()?, active_id: state.active_id()? })
}

/// Opens the most recently used workspace, creating the first one on a fresh install.
pub fn open_initial(state: &AppState) -> AppResult<()> {
    let existing = state.app_db()?.most_recent_workspace()?;
    let id = match existing {
        Some(workspace) => workspace.id,
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            state.app_db()?.create_workspace(&id, DEFAULT_WORKSPACE_NAME)?;
            id
        }
    };
    state.activate(&id)?;
    Ok(())
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> AppResult<WorkspaceList> {
    snapshot(&state)
}

#[tauri::command]
pub async fn create_workspace(app: AppHandle, name: String) -> AppResult<WorkspaceList> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.ensure_unlocked()?;
        let id = uuid::Uuid::new_v4().to_string();
        state.app_db()?.create_workspace(&id, &name)?;
        state.activate(&id)?;
        let list = snapshot(&state)?;
        drop(state);
        crate::commands::automations::resume(&app);
        Ok(list)
    })
    .await
}

#[tauri::command]
pub async fn switch_workspace(app: AppHandle, id: String) -> AppResult<WorkspaceList> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.ensure_unlocked()?;
        if state.active_id()?.as_deref() != Some(id.as_str()) {
            state.activate(&id)?;
        }
        let list = snapshot(&state)?;
        drop(state);
        crate::commands::automations::resume(&app);
        Ok(list)
    })
    .await
}

#[tauri::command]
pub fn rename_workspace(state: State<'_, AppState>, id: String, name: String) -> AppResult<WorkspaceList> {
    state.ensure_unlocked()?;
    state.app_db()?.rename_workspace(&id, &name)?;
    snapshot(&state)
}

/// Deletes a workspace that is not open: its keychain secrets, files and registry entry.
#[tauri::command]
pub async fn delete_workspace(app: AppHandle, id: String) -> AppResult<WorkspaceList> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.ensure_unlocked()?;
        if state.active_id()?.as_deref() == Some(id.as_str()) {
            return Err(AppError::Conflict("Switch to another workspace before deleting this one".into()));
        }
        state.app_db()?.get_workspace(&id)?;
        let dir = state.workspace_dir(&id);
        let db_path = dir.join(WORKSPACE_DB);
        if db_path.is_file() {
            let db = WorkspaceDb::open(&db_path)?;
            for connection in db.list_connections()? {
                if let Some(reference) = connection.secret_ref {
                    secrets::delete(&reference)?;
                }
            }
        }
        state.app_db()?.delete_workspace(&id)?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        snapshot(&state)
    })
    .await
}

pub fn encrypt_active(state: &AppState, id: &str, passphrase: &str, remember: bool) -> AppResult<()> {
    if let Ok(workspace) = state.workspace() {
        let _ = workspace.checkpoint();
    }
    state.close_active()?;
    let src = state.workspace_dir(id).join(WORKSPACE_DB);
    let tmp = state.workspace_dir(id).join("workspace.encrypted.sqlite");
    encrypt_sqlite(&src, &tmp, passphrase)?;
    std::fs::rename(&tmp, &src)?;
    // Remembering the passphrase in the keychain is opt-in. Without it the workspace asks at launch.
    if remember {
        secrets::save(&format!("workspace-key:{id}"), passphrase)?;
    } else {
        secrets::delete(&format!("workspace-key:{id}"))?;
    }
    state.app_db()?.set_encrypted(id, true)?;
    state.activate_with_key(id, Some(passphrase))?;
    Ok(())
}

fn encrypt_sqlite(src: &std::path::Path, dest: &std::path::Path, passphrase: &str) -> AppResult<()> {
    let conn = Connection::open(src)?;
    let cipher: Result<String, _> = conn.query_row("PRAGMA cipher_version", [], |row| row.get(0));
    if cipher.is_err() {
        return Err(AppError::invalid("This build cannot encrypt workspaces (SQLCipher is unavailable)."));
    }
    let dest_sql = dest.to_string_lossy().replace('\'', "''");
    let key_sql = passphrase.replace('\'', "''");
    conn.execute_batch(&format!("ATTACH DATABASE '{dest_sql}' AS enc KEY '{key_sql}';"))?;
    conn.query_row("SELECT sqlcipher_export('enc')", [], |_| Ok(()))?;
    conn.execute("DETACH DATABASE enc", [])?;
    Ok(())
}
