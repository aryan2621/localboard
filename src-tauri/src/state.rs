use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use tokio::sync::oneshot;

use crate::connectors::watcher::CatalogWatcher;
use crate::engine::catalog;
use crate::engine::duck::Analytics;
use crate::engine::mask::AccessPolicy;
use crate::engine::query::CancelToken;
use crate::error::{AppError, AppResult};
use crate::llm::LlmRuntime;
use crate::storage::app_db::AppDb;
use crate::storage::secrets;
use crate::storage::workspace_db::WorkspaceDb;

pub const WORKSPACE_DB: &str = "workspace.sqlite";
pub const ANALYTICS_DB: &str = "analytics.duckdb";
/// The alias an encrypted analytics file is attached under.
const DUCK_CATALOG: &str = "lb_analytics";

fn duck_sql(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn attach_encrypted(conn: &duckdb::Connection, path: &Path, key: &str) -> Result<(), duckdb::Error> {
    conn.execute_batch(&format!(
        "ATTACH {} AS {DUCK_CATALOG} (ENCRYPTION_KEY {}); USE {DUCK_CATALOG};",
        duck_sql(&path.to_string_lossy()),
        duck_sql(key)
    ))
}

/// Opens `analytics.duckdb` as an encrypted catalog on an in-memory host database. A plain file left by an
/// earlier, unencrypted workspace is migrated in place first.
fn open_encrypted_analytics(path: &Path, key: &str, memory_limit_gb: u64) -> AppResult<Analytics> {
    let host = duckdb::Connection::open_in_memory()?;
    if path.is_file() && attach_encrypted(&host, path, key).is_err() {
        // Not openable with this key: either a plain file to migrate, or the wrong key.
        drop(host);
        migrate_plain_analytics(path, key)?;
        let host = duckdb::Connection::open_in_memory()?;
        attach_encrypted(&host, path, key).map_err(|error| AppError::Conflict(format!("The analytics store would not open with this passphrase: {error}")))?;
        return Ok(Analytics::from_connection(host, memory_limit_gb)?);
    }
    if !path.is_file() {
        attach_encrypted(&host, path, key)?;
    }
    Ok(Analytics::from_connection(host, memory_limit_gb)?)
}

/// Copies a plain DuckDB file into a new encrypted one, then swaps it in. The plain original is kept as
/// `.plain.bak` until the encrypted copy has been verified, then removed.
fn migrate_plain_analytics(path: &Path, key: &str) -> AppResult<()> {
    let plain = duckdb::Connection::open(path).map_err(|_| {
        AppError::Conflict("The analytics store cannot be opened with this passphrase, and it is not a plain database either.".into())
    })?;
    let source: String = plain.query_row("SELECT current_database()", [], |row| row.get(0))?;
    let tmp = path.with_extension("duckdb.encrypted");
    let bak = path.with_extension("duckdb.plain.bak");
    let _ = std::fs::remove_file(&tmp);
    let copied = plain.execute_batch(&format!(
        "ATTACH {} AS lb_enc (ENCRYPTION_KEY {}); COPY FROM DATABASE {} TO lb_enc; DETACH lb_enc;",
        duck_sql(&tmp.to_string_lossy()),
        duck_sql(key),
        crate::engine::duck::quote_ident(&source)
    ));
    if let Err(error) = copied {
        drop(plain);
        let _ = std::fs::remove_file(&tmp);
        return Err(error.into());
    }
    let _ = plain.execute_batch("CHECKPOINT;");
    drop(plain);
    // Verify the encrypted copy opens before touching the original.
    {
        let check = duckdb::Connection::open_in_memory()?;
        if let Err(error) = attach_encrypted(&check, &tmp, key) {
            let _ = std::fs::remove_file(&tmp);
            return Err(AppError::Conflict(format!("The encrypted analytics copy failed verification, so nothing was changed: {error}")));
        }
    }
    let _ = std::fs::remove_file(&bak);
    std::fs::rename(path, &bak)?;
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::rename(&bak, path);
        return Err(error.into());
    }
    let _ = std::fs::remove_file(path.with_extension("duckdb.wal"));
    let _ = std::fs::remove_file(&bak);
    Ok(())
}

/// An open workspace: its metadata store, analytics engine and file watches.
pub struct Workspace {
    pub id: String,
    db: Mutex<WorkspaceDb>,
    duck: Mutex<Analytics>,
    /// Spare handle on the same DuckDB instance, used only to mint connections without taking the catalog lock.
    duck_source: Mutex<duckdb::Connection>,
    /// The attached, encrypted analytics catalog, when the workspace is encrypted. Every connection must `USE` it.
    duck_catalog: Option<&'static str>,
    /// Serializes catalog mutations (relation naming, registration, relationship detection).
    catalog_write: Mutex<()>,
    watcher: Mutex<Option<CatalogWatcher>>,
    policy: Mutex<AccessPolicy>,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> AppError {
    AppError::Internal("a workspace lock was poisoned by an earlier failure; restart LocalBoard".into())
}

impl Workspace {
    pub fn open_with_key(id: &str, dir: &Path, memory_limit_gb: u64, key: Option<&str>) -> AppResult<Self> {
        std::fs::create_dir_all(dir)?;
        let analytics = match key {
            Some(key) => open_encrypted_analytics(&dir.join(ANALYTICS_DB), key, memory_limit_gb)?,
            None => Analytics::open(&dir.join(ANALYTICS_DB), memory_limit_gb)?,
        };
        let duck_source = analytics.connection().try_clone()?;
        Ok(Self {
            id: id.to_owned(),
            db: Mutex::new(WorkspaceDb::open_with_key(&dir.join(WORKSPACE_DB), key)?),
            duck: Mutex::new(analytics),
            duck_source: Mutex::new(duck_source),
            duck_catalog: key.map(|_| DUCK_CATALOG),
            catalog_write: Mutex::new(()),
            watcher: Mutex::new(None),
            policy: Mutex::new(AccessPolicy::open()),
        })
    }

    pub fn db(&self) -> AppResult<MutexGuard<'_, WorkspaceDb>> {
        self.db.lock().map_err(poisoned)
    }

    pub fn duck(&self) -> AppResult<MutexGuard<'_, Analytics>> {
        self.duck.lock().map_err(poisoned)
    }

    /// A new connection on the workspace's DuckDB instance. Clones share the instance (and so the attached
    /// catalogs) but each has its own default catalog, so the encrypted one is selected here.
    fn clone_connection(&self) -> AppResult<duckdb::Connection> {
        let conn = self.duck_source.lock().map_err(poisoned)?.try_clone()?;
        if let Some(catalog) = self.duck_catalog {
            conn.execute_batch(&format!("USE {catalog};"))?;
        }
        Ok(conn)
    }

    /// A read connection that does not hold the catalog DuckDB mutex.
    pub fn profile_connection(&self) -> AppResult<duckdb::Connection> {
        let conn = self.clone_connection()?;
        conn.execute_batch("SET threads = 1;")?;
        Ok(conn)
    }

    /// A separate connection to the same database, so long analyst queries never hold the catalog lock.
    pub fn query_connection(&self) -> AppResult<duckdb::Connection> {
        crate::engine::mask::activate(self.policy());
        self.clone_connection()
    }

    /// Kept for callers that ask for raw rows. It never bypasses masking: the connection carries the
    /// workspace masks.
    pub fn query_connection_unmasked(&self) -> AppResult<duckdb::Connection> {
        self.query_connection()
    }

    /// For the loopback server and the scheduler, which act for no desktop user: masked, non-admin.
    pub fn service_connection(&self) -> AppResult<duckdb::Connection> {
        crate::engine::mask::activate(self.service_policy());
        self.clone_connection()
    }

    pub fn service_policy(&self) -> AccessPolicy {
        self.policy()
    }

    pub fn policy(&self) -> AccessPolicy {
        self.policy.lock().map(|policy| policy.clone()).unwrap_or_default()
    }

    pub fn set_policy(&self, policy: AccessPolicy) -> AppResult<()> {
        *self.policy.lock().map_err(poisoned)? = policy;
        Ok(())
    }

    /// Flushes SQLite's WAL and DuckDB's write-ahead file so the files on disk are complete.
    pub fn checkpoint(&self) -> AppResult<()> {
        self.db()?.checkpoint()?;
        self.duck()?.connection().execute_batch("CHECKPOINT;")?;
        Ok(())
    }

    pub fn catalog_guard(&self) -> AppResult<MutexGuard<'_, ()>> {
        self.catalog_write.lock().map_err(poisoned)
    }

    pub fn set_watcher(&self, watcher: Option<CatalogWatcher>) -> AppResult<()> {
        *self.watcher.lock().map_err(poisoned)? = watcher;
        Ok(())
    }
}

pub type ChangeNotifier = Arc<dyn Fn(&str) + Send + Sync>;

/// DuckDB may use this share of memory; the rest stays available for the local model.
const QUERY_MEMORY_DIVISOR: u64 = 2;

/// The answer currently being written. llama-server serves one request at a time, so one
/// analysis runs at a time across the app.
#[derive(Default)]
pub struct AnalystRuns {
    current: Mutex<Option<(String, Arc<CancelToken>)>>,
}

impl AnalystRuns {
    pub fn begin(&self, message_id: &str) -> AppResult<Arc<CancelToken>> {
        let mut current = self.current.lock().map_err(poisoned)?;
        if current.is_some() {
            return Err(AppError::Conflict("An answer is still being written. Stop it or wait for it to finish.".into()));
        }
        let token = Arc::new(CancelToken::default());
        *current = Some((message_id.to_owned(), token.clone()));
        Ok(token)
    }

    pub fn finish(&self, message_id: &str) {
        match self.current.lock() {
            Ok(mut current) if current.as_ref().is_some_and(|(id, _)| id == message_id) => *current = None,
            Ok(_) => {}
            Err(error) => tracing::error!("analyst run registry poisoned: {error}"),
        }
    }

    pub fn cancel(&self, message_id: &str) -> AppResult<()> {
        let current = self.current.lock().map_err(poisoned)?;
        match current.as_ref() {
            Some((id, token)) if id == message_id => {
                token.cancel();
                Ok(())
            }
            _ => Err(AppError::not_found("That answer is no longer running")),
        }
    }
}

#[derive(Default)]
pub struct DashboardRefresh {
    tasks: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl DashboardRefresh {
    pub fn track(&self, id: String, handle: tauri::async_runtime::JoinHandle<()>) {
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(previous) = tasks.insert(id, handle) {
                previous.abort();
            }
        }
    }

    pub fn stop(&self, id: &str) {
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(handle) = tasks.remove(id) {
                handle.abort();
            }
        }
    }

    pub fn stop_all(&self) {
        if let Ok(mut tasks) = self.tasks.lock() {
            for handle in tasks.drain().map(|(_, handle)| handle) {
                handle.abort();
            }
        }
    }
}

pub struct LocalServerHandle {
    pub port: u16,
    pub shutdown: Option<oneshot::Sender<()>>,
}

impl LocalServerHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

pub enum ServerStatus {
    Off,
    On { port: u16 },
    Failed(String),
}

struct ServerSlot {
    status: ServerStatus,
    handle: Option<LocalServerHandle>,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub app_db: Mutex<AppDb>,
    pub llm: Arc<LlmRuntime>,
    pub analyst_runs: AnalystRuns,
    pub refresh: DashboardRefresh,
    pub jobs: DashboardRefresh,
    file_jobs: Mutex<Option<CatalogWatcher>>,
    server: Mutex<ServerSlot>,
    active: RwLock<Option<Arc<Workspace>>>,
    /// Tells the UI a workspace's catalog changed in the background (file watches).
    catalog_changed: ChangeNotifier,
    offline: Mutex<bool>,
    unlocked: Mutex<bool>,
    /// False until workspace open and the loopback server have finished off the UI thread.
    ready: AtomicBool,
}

impl AppState {
    pub fn new(data_dir: PathBuf, app_db: AppDb, llm: Arc<LlmRuntime>, catalog_changed: ChangeNotifier) -> Self {
        Self {
            data_dir,
            app_db: Mutex::new(app_db),
            llm,
            analyst_runs: AnalystRuns::default(),
            refresh: DashboardRefresh::default(),
            jobs: DashboardRefresh::default(),
            file_jobs: Mutex::new(None),
            server: Mutex::new(ServerSlot { status: ServerStatus::Off, handle: None }),
            active: RwLock::new(None),
            catalog_changed,
            offline: Mutex::new(false),
            unlocked: Mutex::new(true),
            ready: AtomicBool::new(false),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn is_offline(&self) -> bool {
        self.offline.lock().map(|flag| *flag).unwrap_or(false)
    }

    pub fn set_offline(&self, enabled: bool) {
        if let Ok(mut flag) = self.offline.lock() {
            *flag = enabled;
        }
    }

    pub fn ensure_online(&self, what: &str) -> AppResult<()> {
        if self.is_offline() {
            Err(AppError::invalid(format!("Offline mode is on. {what} would leave this device.")))
        } else {
            Ok(())
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.lock().map(|flag| *flag).unwrap_or(true)
    }

    pub fn set_unlocked(&self, unlocked: bool) {
        if let Ok(mut flag) = self.unlocked.lock() {
            *flag = unlocked;
        }
    }

    pub fn actor_name(&self) -> String {
        "local".into()
    }

    pub fn refresh_policy(&self) -> AppResult<()> {
        let workspace = self.workspace_unchecked()?;
        let masks = workspace.db()?.list_masks()?;
        workspace.set_policy(AccessPolicy { masks })?;
        workspace.db()?.set_actor(self.actor_name());
        Ok(())
    }

    pub fn set_server(&self, handle: LocalServerHandle) {
        if let Ok(mut slot) = self.server.lock() {
            slot.status = ServerStatus::On { port: handle.port };
            slot.handle = Some(handle);
        }
    }

    pub fn fail_server(&self, message: String) {
        if let Ok(mut slot) = self.server.lock() {
            slot.status = ServerStatus::Failed(message);
            slot.handle = None;
        }
    }

    pub fn shutdown_server(&self) {
        if let Ok(mut slot) = self.server.lock() {
            if let Some(mut handle) = slot.handle.take() {
                handle.stop();
            }
            slot.status = ServerStatus::Off;
        }
    }

    pub fn server_status(&self) -> ServerStatus {
        self.server.lock().map(|slot| match &slot.status {
            ServerStatus::On { port } => ServerStatus::On { port: *port },
            ServerStatus::Failed(message) => ServerStatus::Failed(message.clone()),
            ServerStatus::Off => ServerStatus::Off,
        }).unwrap_or(ServerStatus::Off)
    }

    pub fn server_origin(&self) -> Option<String> {
        match self.server_status() {
            ServerStatus::On { port } => Some(format!("http://127.0.0.1:{port}")),
            _ => None,
        }
    }

    pub fn server_error(&self) -> Option<String> {
        match self.server_status() {
            ServerStatus::Failed(message) => Some(message),
            _ => None,
        }
    }

    pub fn app_db(&self) -> AppResult<MutexGuard<'_, AppDb>> {
        self.app_db.lock().map_err(poisoned)
    }

    pub fn workspace_dir(&self, id: &str) -> PathBuf {
        self.data_dir.join("workspaces").join(id)
    }

    pub fn close_active(&self) -> AppResult<()> {
        self.refresh.stop_all();
        self.jobs.stop_all();
        *self.active.write().map_err(poisoned)? = None;
        Ok(())
    }

    /// The open workspace. Refused while the app lock is engaged, so every command and server route
    /// that reads or changes workspace data is covered by one check.
    pub fn workspace(&self) -> AppResult<Arc<Workspace>> {
        self.ensure_unlocked()?;
        self.workspace_unchecked()
    }

    pub fn ensure_unlocked(&self) -> AppResult<()> {
        if self.is_unlocked() {
            Ok(())
        } else {
            Err(AppError::invalid("LocalBoard is locked. Unlock it first."))
        }
    }

    fn workspace_unchecked(&self) -> AppResult<Arc<Workspace>> {
        self.active
            .read()
            .map_err(poisoned)?
            .clone()
            .ok_or_else(|| AppError::not_found("No workspace is open"))
    }

    pub fn active_id(&self) -> AppResult<Option<String>> {
        Ok(self.active.read().map_err(poisoned)?.as_ref().map(|workspace| workspace.id.clone()))
    }

    /// Opens a workspace (blocking I/O), restores its relations and watches, then makes it active.
    pub fn activate(&self, id: &str) -> AppResult<Arc<Workspace>> {
        self.activate_with_key(id, None)
    }

    /// `passphrase` is used for an encrypted workspace; without one the remembered key is read from the keychain.
    pub fn activate_with_key(&self, id: &str, passphrase: Option<&str>) -> AppResult<Arc<Workspace>> {
        self.app_db()?.get_workspace(id)?;
        let memory_limit_gb = self.llm.hardware().ram_gb / QUERY_MEMORY_DIVISOR;
        let encrypted = self.app_db()?.workspace_encrypted(id)?;
        let key = if encrypted {
            match passphrase {
                Some(passphrase) => Some(passphrase.to_owned()),
                None => Some(secrets::get(&format!("workspace-key:{id}")).map_err(|_| {
                    AppError::Conflict("This workspace is encrypted. Enter its passphrase to open it.".into())
                })?),
            }
        } else {
            None
        };
        let workspace = Arc::new(Workspace::open_with_key(id, &self.workspace_dir(id), memory_limit_gb, key.as_deref())?);
        catalog::restore(&workspace);
        catalog::rewatch(&workspace, self.catalog_changed.clone())?;
        self.refresh.stop_all();
        self.jobs.stop_all();
        *self.active.write().map_err(poisoned)? = Some(workspace.clone());
        self.app_db()?.touch_workspace(id)?;
        let _ = self.refresh_policy();
        Ok(workspace)
    }

    pub fn notifier(&self) -> ChangeNotifier {
        self.catalog_changed.clone()
    }

    pub fn set_file_jobs(&self, watcher: Option<CatalogWatcher>) -> AppResult<()> {
        *self.file_jobs.lock().map_err(poisoned)? = watcher;
        Ok(())
    }
}
