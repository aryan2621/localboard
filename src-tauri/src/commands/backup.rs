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
    if let Err(error) = rekey_connection_secrets(&state, &id) {
        tracing::error!("could not re-key imported connection secrets: {error}");
    }
    crate::commands::automations::resume(&app);
    let record = state.app_db()?.get_workspace(&id)?;
    Ok(record)
}

/// An imported workspace must not share keychain entries with the one it was exported from: deleting either
/// would otherwise remove the other's passwords. Copy each secret to a ref scoped to the new workspace.
fn rekey_connection_secrets(state: &AppState, workspace_id: &str) -> AppResult<()> {
    let workspace = state.workspace()?;
    let db = workspace.db()?;
    for connection in db.list_connections()? {
        let Some(old) = connection.secret_ref else { continue };
        let fresh = format!("{workspace_id}:{}", connection.id);
        match crate::storage::secrets::get(&old) {
            Ok(secret) => match crate::storage::secrets::save(&fresh, &secret) {
                Ok(()) => db.set_connection_secret_ref(&connection.id, Some(fresh.as_str()))?,
                Err(error) => tracing::error!("could not move the secret for connection {}: {error}", connection.id),
            },
            // Only a secret that truly is not there is dropped; a keychain failure keeps the old ref.
            Err(AppError::NotFound(_)) => db.set_connection_secret_ref(&connection.id, None)?,
            Err(error) => tracing::error!("could not read the secret for connection {}: {error}", connection.id),
        }
    }
    for automation in db.list_automations()? {
        let (old, fresh) = (format!("webhook:{}", automation.id), crate::commands::automations::webhook_token_ref(workspace_id, &automation.id));
        if let Ok(token) = crate::storage::secrets::get(&old) {
            if let Err(error) = crate::storage::secrets::save(&fresh, &token) {
                tracing::error!("could not move the webhook secret for automation {}: {error}", automation.id);
            }
        }
    }
    Ok(())
}

fn write_backup(dir: &Path, name: &str, encrypted: bool, dest: &Path) -> AppResult<()> {
    let result = write_zip(dir, name, encrypted, dest);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn write_zip(dir: &Path, name: &str, encrypted: bool, dest: &Path) -> AppResult<()> {
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

/// Largest database a backup may expand to; guards against zip bombs.
const MAX_ENTRY_BYTES: u64 = 20 * 1024 * 1024 * 1024;

fn read_backup(src: &Path, dest: &Path) -> AppResult<bool> {
    let result = read_zip(src, dest);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(dest);
    }
    result
}

fn read_zip(src: &Path, dest: &Path) -> AppResult<bool> {
    let mut encrypted = false;
    std::fs::create_dir_all(dest)?;
    let file = File::open(src)?;
    let mut zip = ZipArchive::new(file).map_err(zip_err)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(zip_err)?;
        // zip 9 returns Err for names it deems unsafe; skip those entries.
        let Ok(name) = entry.name().map(|n| n.into_owned()) else { continue };
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
        let copied = std::io::copy(&mut (&mut entry).take(MAX_ENTRY_BYTES + 1), &mut out)?;
        if copied > MAX_ENTRY_BYTES {
            return Err(AppError::invalid("That backup is too large to import"));
        }
    }
    if !dest.join(WORKSPACE_DB).is_file() {
        return Err(AppError::invalid("That backup has no workspace database"));
    }
    Ok(encrypted)
}

fn zip_err(error: zip::result::ZipError) -> AppError {
    AppError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("localboard-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backup_round_trips_databases_and_manifest() {
        let src = scratch("bk-src");
        std::fs::write(src.join(WORKSPACE_DB), b"workspace-bytes").unwrap();
        std::fs::write(src.join(ANALYTICS_DB), b"analytics-bytes").unwrap();
        let zip_path = src.join("out.zip");
        write_backup(&src, "Acme", true, &zip_path).unwrap();
        let dest = scratch("bk-dest");
        let encrypted = read_zip(&zip_path, &dest).unwrap();
        assert!(encrypted, "the manifest's encrypted flag must survive");
        assert_eq!(std::fs::read(dest.join(WORKSPACE_DB)).unwrap(), b"workspace-bytes");
        assert_eq!(std::fs::read(dest.join(ANALYTICS_DB)).unwrap(), b"analytics-bytes");
    }

    #[test]
    fn import_skips_path_traversal_and_unknown_entries() {
        let dir = scratch("bk-evil");
        let zip_path = dir.join("evil.zip");
        {
            let mut zip = ZipWriter::new(File::create(&zip_path).unwrap());
            let opts = SimpleFileOptions::default();
            for name in ["../escape.txt", "sub/dir.txt", "..\\win.txt", "random.bin", WORKSPACE_DB] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(b"x").unwrap();
            }
            zip.finish().unwrap();
        }
        let dest = dir.join("dest");
        read_zip(&zip_path, &dest).unwrap();
        assert!(!dir.join("escape.txt").exists());
        assert!(!dest.join("sub").exists());
        assert!(!dest.join("random.bin").exists());
        assert!(dest.join(WORKSPACE_DB).is_file());
    }

    #[test]
    fn import_of_a_non_zip_fails_cleanly() {
        let dir = scratch("bk-bad");
        let path = dir.join("nope.zip");
        std::fs::write(&path, b"not a zip").unwrap();
        assert!(read_backup(&path, &dir.join("dest")).is_err());
    }
}
