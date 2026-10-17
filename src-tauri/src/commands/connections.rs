use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Manager, State};

use crate::connectors::files::{FileFormat, PathConfig};
use crate::connectors::sql::{DbConfig, DbParams, ExploredTable};
use crate::connectors::{self, snapshot, AccessMode, ConnectorKind};
use crate::engine::catalog::{self, SyncReport};
use crate::error::{blocking, AppError, AppResult};
use crate::state::{AppState, Workspace};
use crate::storage::secrets;
use crate::storage::workspace_db::{AuditEntry, AuditQuery, AuditStatus, ConnectionRecord};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub id: String,
    pub kind: ConnectorKind,
    pub name: String,
    /// Non-secret summary: a path for files and folders, `user@host:port/db` for databases.
    pub detail: String,
    pub table_count: usize,
    pub source_missing: bool,
    pub created_at: String,
    pub mode: AccessMode,
    /// Oldest "snapshot as of" time across this connection's tables; None for live connections.
    pub snapshot_at: Option<String>,
    pub snapshot_truncated: bool,
    pub row_cap: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSourceResult {
    pub connection: ConnectionView,
    pub report: SyncReport,
}

fn view(workspace: &Workspace, connection: &ConnectionRecord) -> AppResult<ConnectionView> {
    let table_count = workspace.db()?.tables_for_connection(&connection.id)?.len();
    let mut row_cap = None;
    let (detail, source_missing) = if connection.kind.is_path_based() {
        let config = catalog::path_config(connection)?;
        (config.path.display().to_string(), !config.path.exists())
    } else {
        let params: DbParams = serde_json::from_value(connection.config.clone())?;
        if connection.kind.access_mode() == AccessMode::Snapshot {
            row_cap = Some(DbConfig::from_params(String::new(), connection.kind, params.clone(), String::new()).row_cap());
        }
        let detail = match connection.kind {
            ConnectorKind::Sqlite => params.database,
            ConnectorKind::Bigquery => format!("{}/{}", params.database, params.extra.get("dataset").map_or("", String::as_str)),
            ConnectorKind::Motherduck => format!("md:{}", params.database),
            _ => format!("{}@{}:{}/{}", params.username, params.host, params.port, params.database),
        };
        (detail, false)
    };
    let (mut snapshot_at, mut snapshot_truncated) = (None::<String>, false);
    if connection.kind.access_mode() == AccessMode::Snapshot {
        let relations = workspace.db()?.tables_for_connection(&connection.id)?.into_iter().map(|table| table.relation).collect::<Vec<_>>();
        let infos = snapshot::infos(&*workspace.duck()?, &relations);
        snapshot_at = infos.values().map(|info| info.taken_at.clone()).min();
        snapshot_truncated = infos.values().any(|info| info.truncated);
    }
    Ok(ConnectionView {
        mode: connection.kind.access_mode(),
        snapshot_at,
        snapshot_truncated,
        row_cap,
        id: connection.id.clone(),
        kind: connection.kind,
        name: connection.name.clone(),
        detail,
        table_count,
        source_missing,
        created_at: connection.created_at.clone(),
    })
}

/// Offline mode blocks every connector whose traffic leaves this device.
fn ensure_reachable(offline: &bool, connection: &ConnectionRecord) -> AppResult<()> {
    if connection.kind.is_path_based() || !*offline {
        return Ok(());
    }
    let params: DbParams = serde_json::from_value(connection.config.clone())?;
    ensure_online_for(*offline, connection.kind, &params.host)
}

fn ensure_online_for(offline: bool, kind: ConnectorKind, host: &str) -> AppResult<()> {
    if offline && kind.leaves_device(host) {
        return Err(AppError::invalid(format!("Offline mode is on. Connecting to {} would leave this device.", kind.as_str())));
    }
    Ok(())
}

fn audit(workspace: &Workspace, kind: &str, started: Instant, result: &AppResult<impl Sized>, detail: serde_json::Value) {
    let entry = AuditEntry {
        kind,
        status: if result.is_ok() { AuditStatus::Ok } else { AuditStatus::Error },
        duration_ms: i64::try_from(started.elapsed().as_millis()).ok(),
        query: AuditQuery::default(),
        detail: match result {
            Ok(_) => detail,
            Err(error) => json!({ "context": detail, "error": error.to_string() }),
        },
    };
    if let Err(error) = workspace.db().and_then(|db| db.record_audit(&entry)) {
        tracing::error!("could not write audit entry {kind}: {error}");
    }
}

#[tauri::command]
pub async fn list_connections(state: State<'_, AppState>) -> AppResult<Vec<ConnectionView>> {
    let workspace = state.workspace()?;
    blocking(move || {
        let connections = workspace.db()?.list_connections()?;
        connections.iter().map(|connection| view(&workspace, connection)).collect()
    })
    .await
}

/// Connects a data file or a watched folder. A source that cannot be read is not saved.
#[tauri::command]
pub async fn add_file_source(app: AppHandle, path: String, recursive: bool) -> AppResult<AddSourceResult> {
    let added = blocking({
        let app = app.clone();
        move || {
            let state = app.state::<AppState>();
            state.ensure_unlocked()?;
            let workspace = state.workspace()?;
            let started = Instant::now();
            let result = add_path(&workspace, PathBuf::from(&path), recursive);
            audit(&workspace, "source.add", started, &result, json!({ "path": path }));
            let added = result?;
            catalog::rewatch(&workspace, state.notifier())?;
            Ok(added)
        }
    })
    .await?;
    let connection_id = added.connection.id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        if let Ok(workspace) = state.workspace() {
            catalog::profile_and_relate(&workspace, &connection_id);
            (state.notifier())(&workspace.id);
        }
    });
    Ok(added)
}

fn add_path(workspace: &Workspace, path: PathBuf, recursive: bool) -> AppResult<AddSourceResult> {
    let path = path.canonicalize().map_err(|error| AppError::invalid(format!("Cannot open {}: {error}", path.display())))?;
    let kind = if path.is_dir() {
        ConnectorKind::Folder
    } else if FileFormat::from_path(&path).is_some() {
        ConnectorKind::File
    } else {
        return Err(AppError::invalid(format!(
            "Unsupported file type. Use one of: {}",
            FileFormat::EXTENSIONS.join(", ")
        )));
    };
    for existing in workspace.db()?.list_connections()?.iter().filter(|connection| connection.kind.is_path_based()) {
        if catalog::path_config(existing)?.path == path {
            return Err(AppError::Conflict(format!("{} is already connected as {}", path.display(), existing.name)));
        }
    }
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    let record = ConnectionRecord {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        name,
        config: serde_json::to_value(PathConfig { path, recursive: recursive && kind == ConnectorKind::Folder })?,
        secret_ref: None,
        created_at: String::new(),
    };
    workspace.db()?.insert_connection(&record)?;
    match catalog::sync_connection(workspace, &record) {
        Ok(report) => {
            let stored = workspace.db()?.get_connection(&record.id)?;
            Ok(AddSourceResult { connection: view(workspace, &stored)?, report })
        }
        Err(error) => {
            catalog::delete_connection(workspace, &record.id)?;
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn refresh_connection(_app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<SyncReport> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let notifier = state.notifier();
    let state_offline = state.is_offline();
    let report = blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || {
            let started = Instant::now();
            let connection = workspace.db()?.get_connection(&id)?;
            let result = ensure_reachable(&state_offline, &connection)
                .and_then(|_| catalog::sync_connection(&workspace, &connection));
            audit(
                &workspace,
                "source.refresh",
                started,
                &result,
                json!({ "connection": connection.name, "kind": connection.kind, "mode": connection.kind.access_mode() }),
            );
            result
        }
    })
    .await?;
    let conn_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        catalog::profile_and_relate(&workspace, &conn_id);
        notifier(&workspace.id);
    });
    Ok(report)
}

#[tauri::command]
pub async fn remove_connection(app: AppHandle, id: String) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.ensure_unlocked()?;
        let workspace = state.workspace()?;
        let started = Instant::now();
        let connection = workspace.db()?.get_connection(&id)?;
        let dependents = {
            let db = workspace.db()?;
            let relations = db.tables_for_connection(&id)?.into_iter().map(|table| table.relation).collect::<Vec<_>>();
            db.dependents_of_relations(&relations)?
        };
        if !dependents.is_empty() {
            let names = dependents.iter().map(|item| item.name.as_str()).collect::<Vec<_>>().join(", ");
            return Err(AppError::Conflict(format!(
                "This connection is used by {} {}: {}. Remove or retarget those first.",
                dependents.len(),
                if dependents.len() == 1 { "item" } else { "items" },
                names
            )));
        }
        let result = (|| {
            if let Some(reference) = &connection.secret_ref {
                secrets::delete(reference)?;
            }
            catalog::delete_connection(&workspace, &id)
        })();
        audit(&workspace, "connection.remove", started, &result, json!({ "connection": connection.name }));
        result?;
        catalog::rewatch(&workspace, state.notifier())
    })
    .await
}

fn audit_detail(config: &DbConfig) -> serde_json::Value {
    json!({
        "kind": config.kind,
        "mode": config.kind.access_mode(),
        "host": config.host,
        "database": config.database,
        "rowCap": (config.kind.access_mode() == AccessMode::Snapshot).then(|| config.row_cap()),
    })
}

async fn test_config(config: DbConfig) -> AppResult<String> {
    let connector = connectors::build(config)?;
    Ok(connector.test().await?)
}

#[tauri::command]
pub async fn test_db_connection(state: State<'_, AppState>, config: DbConfig) -> AppResult<String> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let started = Instant::now();
    let detail = audit_detail(&config);
    let result = match ensure_online_for(state.is_offline(), config.kind, &config.host) {
        Ok(()) => test_config(config).await,
        Err(error) => Err(error),
    };
    blocking(move || {
        audit(&workspace, "connection.test", started, &result, detail);
        result
    })
    .await
}

/// Verifies the connection, stores the secret in the keychain and the rest in the workspace.
#[tauri::command]
pub async fn save_db_connection(state: State<'_, AppState>, config: DbConfig) -> AppResult<ConnectionView> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let started = Instant::now();
    let detail = audit_detail(&config);
    let config = config.normalized()?;
    let params = config.params();
    let (name, kind, password) = (config.name.clone(), config.kind, config.password.clone());
    let tested = match ensure_online_for(state.is_offline(), kind, &config.host) {
        Ok(()) => test_config(config).await,
        Err(error) => Err(error),
    };
    blocking(move || {
        let result = tested.and_then(|_| {
            let id = uuid::Uuid::new_v4().to_string();
            let secret_ref = (!password.is_empty()).then(|| id.clone());
            if let Some(reference) = &secret_ref {
                secrets::save(reference, &password)?;
            }
            let record = ConnectionRecord { id: id.clone(), kind, name, config: serde_json::to_value(params)?, secret_ref: secret_ref.clone(), created_at: String::new() };
            if let Err(error) = workspace.db()?.insert_connection(&record) {
                if let Some(reference) = &secret_ref {
                    if let Err(cleanup) = secrets::delete(reference) {
                        tracing::error!("could not remove orphaned keychain entry {reference}: {cleanup}");
                    }
                }
                return Err(error);
            }
            let stored = workspace.db()?.get_connection(&id)?;
            view(&workspace, &stored)
        });
        audit(&workspace, "connection.save", started, &result, detail);
        result
    })
    .await
}

#[tauri::command]
pub async fn explore_db_connection(state: State<'_, AppState>, id: String) -> AppResult<Vec<ExploredTable>> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let connection = {
        let workspace = workspace.clone();
        blocking(move || workspace.db()?.get_connection(&id)).await?
    };
    if connection.kind.is_path_based() {
        return Err(AppError::invalid("Files and folders are explored in the schema explorer"));
    }
    let params: DbParams = serde_json::from_value(connection.config.clone())?;
    let password = match &connection.secret_ref {
        Some(reference) => {
            let reference = reference.clone();
            blocking(move || secrets::get(&reference)).await?
        }
        None => String::new(),
    };
    ensure_online_for(state.is_offline(), connection.kind, &params.host)?;
    let connector = connectors::build(DbConfig::from_params(connection.name.clone(), connection.kind, params, password))?;
    let started = Instant::now();
    let result = connector.explore().await.map_err(AppError::from);
    let name = connection.name.clone();
    let audited: AppResult<usize> = match &result {
        Ok(tables) => Ok(tables.len()),
        Err(error) => Err(AppError::invalid(error.to_string())),
    };
    blocking(move || {
        audit(&workspace, "connection.explore", started, &audited, json!({ "connection": name }));
        Ok(())
    })
    .await?;
    result
}

/// Registers chosen remote tables as live read-only DuckDB relations (scanner / ATTACH).
#[tauri::command]
pub async fn register_db_tables(state: State<'_, AppState>, id: String, source_refs: Vec<String>) -> AppResult<SyncReport> {
    state.ensure_unlocked()?;
    let workspace = state.workspace()?;
    let offline = state.is_offline();
    blocking(move || {
        let started = Instant::now();
        let connection = workspace.db()?.get_connection(&id)?;
        if connection.kind.is_path_based() {
            return Err(AppError::invalid("Register tables from a database connection"));
        }
        let params: DbParams = serde_json::from_value(connection.config.clone())?;
        ensure_online_for(offline, connection.kind, &params.host)?;
        let mut report = SyncReport::default();
        for source_ref in source_refs {
            match catalog::register_sql_table(&workspace, &connection, &source_ref) {
                Ok(_) => report.tables += 1,
                Err(error) => report.errors.push(format!("{source_ref}: {error}")),
            }
        }
        let result = Ok(report);
        audit(&workspace, "connection.register", started, &result, json!({ "connection": connection.name }));
        result
    })
    .await
}
