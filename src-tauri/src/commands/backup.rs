use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{Manager, State};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::error::{blocking, AppError, AppResult};
use crate::state::{AppState, ANALYTICS_DB, WORKSPACE_DB};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub version: u32,
    pub workspace_name: String,
    pub exported_at: String,
    #[serde(default)]
    pub encrypted: bool,
}

#[derive(serde::Deserialize, Default)]
struct ManifestIn {
    #[serde(default)]
    encrypted: bool,
}

#[tauri::command]
pub async fn backup_export(state: State<'_, AppState>, path: String) -> AppResult<()> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let dir = state.workspace_dir(&workspace.id);
    let record = state.app_db()?.get_workspace(&workspace.id)?;
    blocking(move || {
        // Fold both write-ahead logs into the main files so the copies are complete.
        workspace.checkpoint()?;
        write_backup(&dir, &record.name, record.encrypted, Path::new(&path))
    })
    .await
}

#[tauri::command]
pub async fn backup_import(app: tauri::AppHandle, state: State<'_, AppState>, path: String, name: String, passphrase: Option<String>, remember: Option<bool>) -> AppResult<crate::storage::app_db::WorkspaceRecord> {
    state.ensure_unlocked()?;
    let id = uuid::Uuid::new_v4().to_string();
    let dest = state.workspace_dir(&id);
    let encrypted = blocking({
        let dest = dest.clone();
        move || read_backup(Path::new(&path), &dest)
    })
    .await?;
    if encrypted && passphrase.as_deref().is_none_or(str::is_empty) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(AppError::invalid("That backup is encrypted. Enter its passphrase to import it."));
    }
    state.app_db()?.create_workspace(&id, &name)?;
    if encrypted {
        state.app_db()?.set_encrypted(&id, true)?;
    }
    drop(state);
    let state = app.state::<AppState>();
    if let Err(error) = state.activate_with_key(&id, passphrase.as_deref()) {
        let _ = state.app_db().and_then(|db| db.delete_workspace(&id));
        let _ = std::fs::remove_dir_all(&dest);
        return Err(error);
    }
    if encrypted && remember.unwrap_or(false) {
        crate::storage::secrets::save(&format!("workspace-key:{id}"), passphrase.as_deref().unwrap_or_default())?;
    }
    crate::commands::automations::resume(&app);
    let record = state.app_db()?.get_workspace(&id)?;
    Ok(record)
}

fn write_backup(dir: &Path, name: &str, encrypted: bool, dest: &Path) -> AppResult<()> {
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let manifest = serde_json::to_vec_pretty(&BackupManifest {
        version: 1,
        workspace_name: name.to_owned(),
        exported_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        encrypted,
    })?;
    zip.start_file("manifest.json", opts).map_err(zip_err)?;
    zip.write_all(&manifest)?;
    add_file(&mut zip, dir.join(WORKSPACE_DB), WORKSPACE_DB, opts)?;
    add_file(&mut zip, dir.join(ANALYTICS_DB), ANALYTICS_DB, opts)?;
    zip.finish().map_err(zip_err)?;
    Ok(())
}

fn add_file(zip: &mut ZipWriter<File>, path: PathBuf, name: &str, opts: SimpleFileOptions) -> AppResult<()> {
    if !path.is_file() {
        return Ok(());
    }
    zip.start_file(name, opts).map_err(zip_err)?;
    let mut file = File::open(path)?;
    std::io::copy(&mut file, zip)?;
    Ok(())
}

fn read_backup(src: &Path, dest: &Path) -> AppResult<bool> {
    let mut encrypted = false;
    std::fs::create_dir_all(dest)?;
    let file = File::open(src)?;
    let mut zip = ZipArchive::new(file).map_err(zip_err)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(zip_err)?;
        let name = entry.name().to_owned();
        if name.contains("..") || name.contains('\\') || name.contains('/') && name != "manifest.json" {
            if !matches!(name.as_str(), WORKSPACE_DB | ANALYTICS_DB | "manifest.json") {
                continue;
            }
        }
        if name == "manifest.json" {
            let mut buf = String::new();
            entry.read_to_string(&mut buf)?;
            encrypted = serde_json::from_str::<ManifestIn>(&buf).unwrap_or_default().encrypted;
            continue;
        }
        if name != WORKSPACE_DB && name != ANALYTICS_DB {
            continue;
        }
        let mut out = File::create(dest.join(&name))?;
        std::io::copy(&mut entry, &mut out)?;
    }
    if !dest.join(WORKSPACE_DB).is_file() {
        return Err(AppError::invalid("That backup has no workspace database"));
    }
    Ok(encrypted)
}

fn zip_err(error: zip::result::ZipError) -> AppError {
    AppError::Internal(error.to_string())
}
