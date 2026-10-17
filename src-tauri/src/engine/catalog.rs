//! The unified catalog: keeps DuckDB relations, profiles and relationships in step with sources.

use std::collections::HashSet;
use std::sync::{Arc, Weak};

use serde::Serialize;

use super::profile::profile;
use super::relationships::{self, TableInfo};
use crate::connectors::files::{self, PathConfig};
use crate::connectors::sql::{DbConfig, DbParams};
use crate::connectors::{self, snapshot, AccessMode};
use crate::connectors::watcher::{self, WatchScope, WatchTarget};
use crate::connectors::ConnectorKind;
use crate::error::{AppError, AppResult};
use crate::state::{ChangeNotifier, Workspace};
use crate::storage::secrets;
use crate::storage::workspace_db::{ConnectionRecord, TableRecord};

const MAX_RELATION_LEN: usize = 48;

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub tables: usize,
    pub errors: Vec<String>,
}

pub fn path_config(connection: &ConnectionRecord) -> AppResult<PathConfig> {
    if !connection.kind.is_path_based() {
        return Err(AppError::invalid("Not a file or folder connection"));
    }
    Ok(serde_json::from_value(connection.config.clone())?)
}

/// Discovers the connection's tables, (re)registers and profiles each, and drops tables whose
/// source disappeared. A missing source fails the sync without erasing catalog metadata.
fn db_config(connection: &ConnectionRecord) -> AppResult<DbConfig> {
    let params: DbParams = serde_json::from_value(connection.config.clone())?;
    let password = match &connection.secret_ref {
        Some(reference) => secrets::get(reference)?,
        None => String::new(),
    };
    Ok(DbConfig::from_params(connection.name.clone(), connection.kind, params, password))
}

/// Live connectors get a scanner view; snapshot connectors are fetched first (no DuckDB lock held during
/// network I/O) and then copied in with a "snapshot as of" stamp. Must run on a blocking thread.
fn register_remote(workspace: &Workspace, config: &DbConfig, source_ref: &str, relation: &str) -> Result<(), connectors::ConnectorError> {
    let connector = connectors::build(config.clone())?;
    match connector.mode() {
        AccessMode::Live => {
            let duck = workspace.duck().map_err(|error| connectors::ConnectorError::Failed(error.to_string()))?;
            connector.scan(&duck, source_ref, relation)
        }
        AccessMode::Snapshot => {
            let fetched = tauri::async_runtime::block_on(connector.fetch(source_ref, config.row_cap()))?;
            let duck = workspace.duck().map_err(|error| connectors::ConnectorError::Failed(error.to_string()))?;
            snapshot::load(&duck, relation, &fetched)
        }
    }
}

/// Re-registers live scanner views already in the catalog. Does not invent new tables.
fn sync_sql_connection(workspace: &Workspace, connection: &ConnectionRecord) -> AppResult<SyncReport> {
    let _guard = workspace.catalog_guard()?;
    let config = db_config(connection)?;
    let tables = workspace.db()?.tables_for_connection(&connection.id)?;
    let mut report = SyncReport::default();
    let mut registered = Vec::new();
    for table in tables {
        let outcome = {
            register_remote(workspace, &config, &table.source_ref, &table.relation)
        };
        match outcome {
            Ok(()) => registered.push(table),
            Err(error) => {
                let message = error.to_string();
                workspace.db()?.save_profile_error(&table.id, &message)?;
                report.errors.push(format!("{}: {message}", table.relation));
            }
        }
    }
    drop(_guard);
    report.tables = registered.len();
    Ok(report)
}

/// Adds a live read-only relation for a table the user picked from Explore.
pub fn register_sql_table(workspace: &Workspace, connection: &ConnectionRecord, source_ref: &str) -> AppResult<TableRecord> {
    let source_ref = source_ref.trim();
    if source_ref.is_empty() {
        return Err(AppError::invalid("Choose a table to register"));
    }
    let record = {
        let _guard = workspace.catalog_guard()?;
        let found = workspace.db()?.find_table(&connection.id, source_ref)?;
        if let Some(existing) = found {
            let config = db_config(connection)?;
            register_remote(workspace, &config, source_ref, &existing.relation)?;
            existing
        } else {
            let suggested = source_ref.rsplit('.').next().unwrap_or(source_ref);
            let relation = allocate_relation(workspace, suggested)?;
            let record = workspace.db()?.insert_table(&uuid::Uuid::new_v4().to_string(), &connection.id, source_ref, &relation)?;
            let config = db_config(connection)?;
            if let Err(error) = register_remote(workspace, &config, source_ref, &relation) {
                let _ = workspace.db()?.delete_table(&record.id);
                return Err(error.into());
            }
            record
        }
    };
    match profile_relation(workspace, &record.relation) {
        Ok(profiled) => {
            workspace.db()?.get_table(&record.id)?;
            workspace.db()?.save_profile(&record.id, &profiled)?;
            refresh_relationships(workspace)?;
            workspace.db()?.get_table(&record.id)
        }
        Err(error) => {
            if workspace.db()?.get_table(&record.id).is_ok() {
                let _ = workspace.db()?.delete_table(&record.id);
                let _ = workspace.duck()?.drop_relation(&record.relation);
            }
            Err(error)
        }
    }
}

pub fn sync_connection(workspace: &Workspace, connection: &ConnectionRecord) -> AppResult<SyncReport> {
    if !connection.kind.is_path_based() {
        return sync_sql_connection(workspace, connection);
    }
    let source = files::source_for(connection.kind, &path_config(connection)?)?;
    let discovered = source.discover()?;
    let mut report = SyncReport::default();
    let mut seen = HashSet::new();
    let mut registered = Vec::new();

    {
        let _guard = workspace.catalog_guard()?;
        for table in &discovered {
            seen.insert(table.source_ref.clone());
            let found = workspace.db()?.find_table(&connection.id, &table.source_ref)?;
            let record = match found {
                Some(record) => record,
                None => {
                    let relation = allocate_relation(workspace, &table.suggested_name)?;
                    workspace
                        .db()?
                        .insert_table(&uuid::Uuid::new_v4().to_string(), &connection.id, &table.source_ref, &relation)?
                }
            };
            match {
                let duck = workspace.duck()?;
                source.register(&duck, table, &record.relation)
            } {
                Ok(()) => registered.push(record),
                Err(error) => {
                    let message = error.to_string();
                    workspace.db()?.save_profile_error(&record.id, &message)?;
                    report.errors.push(format!("{}: {message}", record.relation));
                }
            }
        }

        // A file that is on disk but failed to open (e.g. locked mid-save) keeps its table.
        let discovered_files: HashSet<&str> =
            discovered.iter().map(|table| table.source_ref.split('#').next().unwrap_or_default()).collect();
        let unreadable = |source_ref: &str| {
            let file = source_ref.split('#').next().unwrap_or_default();
            !discovered_files.contains(file) && std::path::Path::new(file).exists()
        };
        let stale: Vec<TableRecord> = workspace
            .db()?
            .tables_for_connection(&connection.id)?
            .into_iter()
            .filter(|table| !seen.contains(&table.source_ref) && !unreadable(&table.source_ref))
            .collect();
        for table in stale {
            workspace.duck()?.drop_relation(&table.relation)?;
            workspace.db()?.delete_table(&table.id)?;
        }
    }

    report.tables = registered.len();
    Ok(report)
}

/// Profiles tables and refreshes relationships. Intended for background execution after
/// `sync_connection` returns so the UI is not blocked by column stats and FK detection.
pub fn profile_and_relate(workspace: &Workspace, connection_id: &str) {
    let tables = match workspace.db().and_then(|db| db.tables_for_connection(connection_id)) {
        Ok(tables) => tables,
        Err(error) => {
            tracing::warn!("deferred profiling: cannot load tables for {connection_id}: {error}");
            return;
        }
    };
    let mut dummy = SyncReport::default();
    for table in &tables {
        save_profile_or_error(workspace, table, &mut dummy);
    }
    if let Err(error) = refresh_relationships(workspace) {
        tracing::warn!("deferred relationship refresh failed: {error}");
    }
}

fn save_profile_or_error(workspace: &Workspace, table: &TableRecord, report: &mut SyncReport) {
    if workspace.db().and_then(|db| db.get_table(&table.id)).is_err() {
        return;
    }
    match profile_relation(workspace, &table.relation) {
        Ok(profiled) => {
            if workspace.db().and_then(|mut db| db.save_profile(&table.id, &profiled)).is_ok() {
                report.tables += 1;
            }
        }
        Err(error) => {
            let message = error.to_string();
            let _ = workspace.db().and_then(|db| db.save_profile_error(&table.id, &message));
            report.errors.push(format!("{}: {message}", table.relation));
        }
    }
}

/// Profiles on a separate DuckDB connection so add/delete are not blocked on CSV scans.
fn profile_relation(workspace: &Workspace, relation: &str) -> AppResult<super::profile::TableProfile> {
    let conn = workspace.profile_connection()?;
    profile(&conn, relation).map_err(|error| match error {
        duckdb::Error::QueryReturnedNoRows => AppError::invalid("Profiling this table took more than 15 seconds and was stopped."),
        other => other.into(),
    })
}

/// Removes the connection's relations and catalog rows. Secrets are the caller's responsibility.
pub fn delete_connection(workspace: &Workspace, connection_id: &str) -> AppResult<()> {
    let _guard = workspace.catalog_guard()?;
    let tables = workspace.db()?.tables_for_connection(connection_id)?;
    {
        let duck = workspace.duck()?;
        for table in &tables {
            duck.drop_relation(&table.relation)?;
        }
    }
    workspace.db()?.delete_connection(connection_id)?;
    Ok(())
}

/// Renames the SQL name of a table. Kept consistent across DuckDB and the catalog.
pub fn rename_table(workspace: &Workspace, table_id: &str, new_name: &str) -> AppResult<TableRecord> {
    let new_name = new_name.trim();
    validate_relation_name(new_name)?;
    let _guard = workspace.catalog_guard()?;
    let table = workspace.db()?.get_table(table_id)?;
    if table.relation == new_name {
        return Ok(table);
    }
    if !table.relation.eq_ignore_ascii_case(new_name) && is_taken(workspace, new_name)? {
        return Err(AppError::Conflict(format!("A table named {new_name} already exists")));
    }
    workspace.duck()?.rename_relation(&table.relation, new_name)?;
    if let Err(error) = workspace.db()?.set_table_relation(table_id, new_name) {
        workspace.duck()?.rename_relation(new_name, &table.relation)?;
        return Err(error);
    }
    if let Err(error) = workspace.db()?.rename_query_relation(&table.relation, new_name) {
        tracing::warn!("could not rename lineage for {}: {error}", table.relation);
    }
    workspace.db()?.get_table(table_id)
}

/// Re-registers any catalog table whose DuckDB relation is missing (for example after the
/// analytics file was removed). Failures are recorded on the table, never fatal to opening.
pub fn restore(workspace: &Workspace) {
    let result = (|| -> AppResult<()> {
        let tables = workspace.db()?.list_tables()?;
        let mut pending = HashSet::new();
        {
            let duck = workspace.duck()?;
            for table in &tables {
                if !duck.relation_exists(&table.relation)? {
                    pending.insert(table.connection_id.clone());
                }
            }
        }
        for connection_id in pending {
            let connection = workspace.db()?.get_connection(&connection_id)?;
            if connection.kind.access_mode() == AccessMode::Snapshot {
                // Never reach out at startup; the user refreshes snapshots explicitly.
                continue;
            }
            if let Err(error) = sync_connection(workspace, &connection) {
                tracing::warn!("could not restore connection {}: {error}", connection.name);
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!("workspace {} restore failed: {error}", workspace.id);
    }
}

/// Rebuilds file watches for every file and folder connection of the workspace.
pub fn rewatch(workspace: &Arc<Workspace>, notify: ChangeNotifier) -> AppResult<()> {
    let targets: Vec<WatchTarget> = workspace
        .db()?
        .list_connections()?
        .into_iter()
        .filter_map(|connection| {
            let config = path_config(&connection).ok()?;
            let scope = match connection.kind {
                ConnectorKind::File => WatchScope::File,
                ConnectorKind::Folder => WatchScope::Folder { recursive: config.recursive },
                _ => return None,
            };
            Some(WatchTarget { connection_id: connection.id, path: config.path, scope })
        })
        .collect();
    if targets.is_empty() {
        return workspace.set_watcher(None);
    }
    let weak: Weak<Workspace> = Arc::downgrade(workspace);
    let watcher = watcher::start(targets, move |connection_ids| {
        let Some(workspace) = weak.upgrade() else { return };
        for id in connection_ids {
            let synced = workspace.db().and_then(|db| db.get_connection(&id)).and_then(|connection| sync_connection(&workspace, &connection));
            if let Err(error) = synced {
                tracing::warn!("refresh after file change failed for {id}: {error}");
            }
        }
        notify(&workspace.id);
    })
    .map_err(|error| AppError::Internal(format!("file watcher: {error}")))?;
    workspace.set_watcher(Some(watcher))
}

fn refresh_relationships(workspace: &Workspace) -> AppResult<()> {
    let tables = workspace.db()?.list_tables()?;
    let profiled: Vec<(TableRecord, Vec<super::profile::ColumnProfile>)> = {
        let db = workspace.db()?;
        tables
            .into_iter()
            .filter(|table| table.profile_error.is_none() && table.row_count.is_some())
            .map(|table| {
                let columns = db.columns(&table.id)?.into_iter().filter_map(|column| column.profile).collect();
                Ok((table, columns))
            })
            .collect::<AppResult<_>>()?
    };
    let infos: Vec<TableInfo<'_>> = profiled
        .iter()
        .map(|(table, columns)| TableInfo {
            id: &table.id,
            relation: &table.relation,
            row_count: table.row_count.unwrap_or(0),
            columns,
        })
        .collect();
    let conn = workspace.profile_connection()?;
    let detected = relationships::detect(&conn, &infos);
    workspace.db()?.replace_detected_relationships(&detected)
}

fn is_taken(workspace: &Workspace, relation: &str) -> AppResult<bool> {
    Ok(workspace.db()?.relation_taken(relation)? || workspace.duck()?.relation_exists(relation)?)
}

fn allocate_relation(workspace: &Workspace, suggested: &str) -> AppResult<String> {
    let base = slugify(suggested);
    let mut candidate = base.clone();
    let mut suffix = 2u32;
    while is_taken(workspace, &candidate)? {
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
    Ok(candidate)
}

/// Turns a file or sheet name into a SQL-friendly relation name the LLM and users can type.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for ch in name.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let mut slug = slug.trim_matches('_').to_owned();
    if slug.is_empty() {
        slug = "table".into();
    }
    if slug.starts_with(|ch: char| ch.is_ascii_digit()) {
        slug.insert_str(0, "t_");
    }
    slug.truncate(MAX_RELATION_LEN);
    slug.trim_end_matches('_').to_owned()
}

fn validate_relation_name(name: &str) -> AppResult<()> {
    let mut chars = name.chars();
    let valid = name.len() <= MAX_RELATION_LEN
        && chars.next().is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && !name.ends_with("__loading");
    if !valid {
        return Err(AppError::invalid(format!(
            "Table names use lowercase letters, digits and underscores, start with a letter, and are at most {MAX_RELATION_LEN} characters"
        )));
    }
    Ok(())
}
