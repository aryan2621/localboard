use serde::{Deserialize, Serialize};
use tauri::State;

use crate::engine::mask::{ColumnMask, MaskStrategy};
use crate::error::{blocking, AppError, AppResult};
use crate::state::AppState;
use crate::storage::security::tokens_match;
use crate::storage::governance::hash_secret;

const LOCK_KEY: &str = "lock.enabled";
const LOCK_SALT: &str = "lock.salt";
const LOCK_HASH: &str = "lock.hash";
const OFFLINE_KEY: &str = "offline.enabled";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GovernanceStatus {
    pub locked: bool,
    pub lock_enabled: bool,
    pub offline: bool,
    pub encrypted: bool,
    /// What encryption covers in this build, for the Settings text.
    pub encryption_scope: &'static str,
}

const ENCRYPTION_SCOPE: &str = "Encrypts the workspace database (conversations, dashboards, settings, audit log) and the analytics store \
(analytics.duckdb, which holds imported data) with your passphrase. Source files you point LocalBoard at, exports and backups you save, \
and data in transit are not covered; keep those on a disk with full-disk encryption.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskDraft {
    pub relation: String,
    pub column_name: String,
    pub strategy: String,
}

#[tauri::command]
pub fn governance_status(state: State<'_, AppState>) -> AppResult<GovernanceStatus> {
    let lock_enabled = state.app_db()?.setting::<bool>(LOCK_KEY)?.unwrap_or(false);
    let offline = state.app_db()?.setting::<bool>(OFFLINE_KEY)?.unwrap_or(false);
    let workspace = state.workspace().ok();
    let encrypted = match &workspace {
        Some(workspace) => state.app_db()?.workspace_encrypted(&workspace.id)?,
        None => false,
    };
    Ok(GovernanceStatus {
        locked: lock_enabled && !state.is_unlocked(),
        lock_enabled,
        offline,
        encrypted,
        encryption_scope: ENCRYPTION_SCOPE,
    })
}

#[tauri::command]
pub fn set_offline(state: State<'_, AppState>, enabled: bool) -> AppResult<bool> {
    state.ensure_unlocked()?;
    state.app_db()?.set_setting(OFFLINE_KEY, &enabled)?;
    state.set_offline(enabled);
    Ok(enabled)
}

#[tauri::command]
pub fn set_app_lock(state: State<'_, AppState>, enabled: bool, passphrase: String, current: Option<String>) -> AppResult<bool> {
    state.ensure_unlocked()?;
    if enabled {
        if passphrase.chars().count() < 8 {
            return Err(AppError::invalid("The app lock needs at least 8 characters"));
        }
        // Replacing an existing lock passphrase needs the current one.
        let salt: Option<String> = state.app_db()?.setting(LOCK_SALT)?;
        let hash: Option<String> = state.app_db()?.setting(LOCK_HASH)?;
        if let (Some(salt), Some(hash)) = (salt, hash) {
            let current = current.unwrap_or_default();
            if !tokens_match(&hash_secret(&salt, &current), &hash) {
                return Err(AppError::invalid("Enter the current lock passphrase to change it"));
            }
        }
        let salt = uuid::Uuid::new_v4().simple().to_string();
        state.app_db()?.set_setting(LOCK_SALT, &salt)?;
        state.app_db()?.set_setting(LOCK_HASH, &hash_secret(&salt, &passphrase))?;
        state.app_db()?.set_setting(LOCK_KEY, &true)?;
        state.set_unlocked(true);
    } else {
        let salt: Option<String> = state.app_db()?.setting(LOCK_SALT)?;
        let hash: Option<String> = state.app_db()?.setting(LOCK_HASH)?;
        if let (Some(salt), Some(hash)) = (salt, hash) {
            if !tokens_match(&hash_secret(&salt, &passphrase), &hash) {
                return Err(AppError::invalid("Wrong passphrase"));
            }
        }
        state.app_db()?.set_setting(LOCK_KEY, &false)?;
        state.set_unlocked(true);
    }
    Ok(enabled)
}

#[tauri::command]
pub fn unlock_app(state: State<'_, AppState>, passphrase: String) -> AppResult<bool> {
    let salt: String = state.app_db()?.setting(LOCK_SALT)?.ok_or_else(|| AppError::invalid("No app lock is set"))?;
    let hash: String = state.app_db()?.setting(LOCK_HASH)?.ok_or_else(|| AppError::invalid("No app lock is set"))?;
    if !tokens_match(&hash_secret(&salt, &passphrase), &hash) {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        return Err(AppError::invalid("Wrong passphrase"));
    }
    state.set_unlocked(true);
    Ok(true)
}

/// Locks the app now. The backend refuses workspace commands and local server routes until unlock_app succeeds.
#[tauri::command]
pub fn lock_app(state: State<'_, AppState>) -> AppResult<bool> {
    let enabled = state.app_db()?.setting::<bool>(LOCK_KEY)?.unwrap_or(false);
    if enabled {
        state.set_unlocked(false);
    }
    Ok(enabled)
}

#[tauri::command]
pub async fn mask_list(state: State<'_, AppState>) -> AppResult<Vec<ColumnMask>> {
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.list_masks()).await
}

#[tauri::command]
pub async fn mask_save(state: State<'_, AppState>, draft: MaskDraft) -> AppResult<ColumnMask> {
    state.ensure_unlocked()?;
    let strategy = match draft.strategy.as_str() {
        "hash" => MaskStrategy::Hash,
        "email" => MaskStrategy::Email,
        "redact" => MaskStrategy::Redact,
        _ => return Err(AppError::invalid("Strategy must be redact, hash or email")),
    };
    let workspace = state.workspace()?;
    let mask = blocking(move || {
        workspace.db()?.upsert_mask(&ColumnMask {
            id: uuid::Uuid::new_v4().to_string(),
            relation: draft.relation,
            column_name: draft.column_name,
            strategy,
        })
    })
    .await?;
    state.refresh_policy()?;
    Ok(mask)
}

#[tauri::command]
pub async fn mask_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    blocking(move || workspace.db()?.delete_mask(&id)).await?;
    state.refresh_policy()
}

#[tauri::command]
pub async fn encrypt_workspace(state: State<'_, AppState>, passphrase: String, remember: Option<bool>) -> AppResult<bool> {
    state.ensure_unlocked()?;
    if passphrase.chars().count() < 8 {
        return Err(AppError::invalid("The encryption passphrase needs at least 8 characters"));
    }
    let workspace = state.workspace()?;
    let id = workspace.id.clone();
    if state.app_db()?.workspace_encrypted(&id)? {
        return Err(AppError::invalid("This workspace is already encrypted"));
    }
    drop(workspace);
    crate::commands::workspaces::encrypt_active(&state, &id, &passphrase, remember.unwrap_or(false))?;
    Ok(true)
}

/// Opens an encrypted workspace with its passphrase. `remember` stores the key in the OS keychain
/// so later launches open it without asking; otherwise nothing is stored.
#[tauri::command]
pub async fn unlock_workspace(app: tauri::AppHandle, id: String, passphrase: String, remember: Option<bool>) -> AppResult<bool> {
    blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        state.ensure_unlocked()?;
        state.activate_with_key(&id, Some(&passphrase))?;
        if remember.unwrap_or(false) {
            crate::storage::secrets::save(&format!("workspace-key:{id}"), &passphrase)?;
        }
        drop(state);
        crate::commands::automations::resume(&app);
        Ok(true)
    })
    .await
}

pub fn restore_lock_and_offline(state: &AppState) {
    let lock_enabled = state.app_db().ok().and_then(|db| db.setting::<bool>(LOCK_KEY).ok().flatten()).unwrap_or(false);
    state.set_unlocked(!lock_enabled);
    let offline = state.app_db().ok().and_then(|db| db.setting::<bool>(OFFLINE_KEY).ok().flatten()).unwrap_or(false);
    state.set_offline(offline);
}
