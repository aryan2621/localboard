use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::provider::LlmError;

const BINARY_NAME: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
const STARTUP_DEADLINE: Duration = Duration::from_secs(120);
const HEALTH_POLL: Duration = Duration::from_millis(500);
const STOP_GRACE: Duration = Duration::from_secs(5);
const CONTEXT_SIZE: &str = "16384";
const EMBED_CONTEXT: &str = "2048";

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,
}

struct Running {
    child: Child,
    endpoint: Endpoint,
}

struct Slot {
    running: Mutex<Option<Running>>,
    pid: AtomicU32,
    log_file: &'static str,
    pid_file: &'static str,
}

impl Slot {
    fn new(log_file: &'static str, pid_file: &'static str) -> Self {
        Self { running: Mutex::new(None), pid: AtomicU32::new(0), log_file, pid_file }
    }
}

/// Owns the bundled llama.cpp processes: one chat model and, separately, one embedding model.
/// Both listen on loopback only, each with its own per-launch API key.
pub struct Supervisor {
    binary: Option<PathBuf>,
    data_dir: PathBuf,
    client: reqwest::Client,
    chat: Slot,
    embed: Slot,
}

impl Supervisor {
    pub fn new(data_dir: PathBuf, binary: Option<PathBuf>) -> Result<Self, LlmError> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build()?;
        Ok(Self {
            binary,
            data_dir,
            client,
            chat: Slot::new("llama-server.log", "llama-server.pid"),
            embed: Slot::new("llama-embed.log", "llama-embed.pid"),
        })
    }

    /// `LLAMA_SERVER_PATH`, else next to the app executable (where Tauri places `externalBin`).
    pub fn locate_binary() -> Option<PathBuf> {
        if let Some(explicit) = std::env::var_os("LLAMA_SERVER_PATH").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(explicit)).filter(|path| path.is_file());
        }
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        [dir.join(BINARY_NAME), dir.join("llama").join(BINARY_NAME)].into_iter().find(|path| path.is_file())
    }

    pub fn binary_available(&self) -> bool {
        self.binary.is_some()
    }

    pub async fn current(&self) -> Option<Endpoint> {
        self.alive(&self.chat).await
    }

    pub async fn embed_model(&self) -> Option<String> {
        self.alive(&self.embed).await.map(|endpoint| endpoint.model_id)
    }

    /// Starts (or reuses) the chat server for `model_id` and waits until it answers `/health`.
    pub async fn ensure(&self, model_id: &str, model_path: &Path) -> Result<Endpoint, LlmError> {
        self.ensure_in(&self.chat, model_id, model_path, false).await
    }

    /// Starts the embedding server. It stays up alongside the chat model; it is a few hundred MB.
    pub async fn ensure_embed(&self, model_id: &str, model_path: &Path) -> Result<Endpoint, LlmError> {
        self.ensure_in(&self.embed, model_id, model_path, true).await
    }

    pub async fn stop(&self) {
        self.stop_slot(&self.chat).await;
        self.stop_slot(&self.embed).await;
    }

    pub async fn stop_chat(&self) {
        self.stop_slot(&self.chat).await;
    }

    pub async fn stop_embed(&self) {
        self.stop_slot(&self.embed).await;
    }

    /// Synchronous kill for app exit, when the async runtime may already be shutting down.
    pub fn kill_now(&self) {
        self.kill_slot(&self.chat);
        self.kill_slot(&self.embed);
    }

    /// Stops servers an earlier, force-killed LocalBoard left running, so they do not hold models in memory.
    pub fn stop_leftover(&self) {
        self.stop_leftover_slot(&self.chat);
        self.stop_leftover_slot(&self.embed);
    }

    async fn alive(&self, slot: &Slot) -> Option<Endpoint> {
        let mut running = slot.running.lock().await;
        let live = matches!(running.as_mut().map(|state| state.child.try_wait()), Some(Ok(None)));
        if !live {
            *running = None;
        }
        running.as_ref().map(|state| state.endpoint.clone())
    }

    async fn ensure_in(&self, slot: &Slot, model_id: &str, model_path: &Path, embeddings: bool) -> Result<Endpoint, LlmError> {
        let mut running = slot.running.lock().await;
        if let Some(state) = running.as_mut() {
            if state.endpoint.model_id == model_id && matches!(state.child.try_wait(), Ok(None)) {
                return Ok(state.endpoint.clone());
            }
        }
        self.stop_locked(slot, &mut running).await;
        self.stop_leftover_slot(slot);

        let binary = self
            .binary
            .as_ref()
            .ok_or_else(|| LlmError::Runtime("the built-in AI runtime (llama-server) is missing from this build".into()))?;
        let port = free_port()?;
        let api_key = uuid::Uuid::new_v4().simple().to_string();
        std::fs::create_dir_all(&self.data_dir).map_err(|error| LlmError::Runtime(error.to_string()))?;
        let log_path = self.data_dir.join(slot.log_file);
        let log = std::fs::File::create(&log_path).map_err(|error| LlmError::Runtime(error.to_string()))?;
        let log_err = log.try_clone().map_err(|error| LlmError::Runtime(error.to_string()))?;

        let mut command = Command::new(binary);
        command
            .arg("--model")
            .arg(model_path)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--api-key", &api_key])
            .args(["--n-gpu-layers", "999", "--parallel", "1"])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .kill_on_drop(true);
        if embeddings {
            command.args(["--embeddings", "--pooling", "mean", "--ctx-size", EMBED_CONTEXT]);
        } else {
            command.args(["--ctx-size", CONTEXT_SIZE, "--jinja", "--reasoning-budget", "0"]);
        }
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = command.spawn().map_err(|error| LlmError::Runtime(format!("could not start llama-server: {error}")))?;
        let pid = child.id().unwrap_or(0);
        slot.pid.store(pid, Ordering::SeqCst);
        let _ = std::fs::write(self.data_dir.join(slot.pid_file), pid.to_string());

        let endpoint = Endpoint { base_url: format!("http://127.0.0.1:{port}/v1"), api_key, model_id: model_id.to_owned() };
        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + STARTUP_DEADLINE;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = child.try_wait() {
                self.forget_pid(slot);
                let reason = last_error(&log_path).unwrap_or_else(|| format!("it exited with {status}"));
                return Err(LlmError::Runtime(format!("the AI model could not start: {reason}")));
            }
            let ready = self
                .client
                .get(&health)
                .bearer_auth(&endpoint.api_key)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success());
            if ready {
                tracing::info!("llama-server ready with {model_id} on port {port}");
                *running = Some(Running { child, endpoint: endpoint.clone() });
                return Ok(endpoint);
            }
            tokio::time::sleep(HEALTH_POLL).await;
        }
        let _ = child.kill().await;
        self.forget_pid(slot);
        Err(LlmError::Runtime("the AI model took too long to load; try again or pick a smaller model".into()))
    }

    async fn stop_slot(&self, slot: &Slot) {
        let mut running = slot.running.lock().await;
        self.stop_locked(slot, &mut running).await;
    }

    async fn stop_locked(&self, slot: &Slot, running: &mut Option<Running>) {
        let Some(mut state) = running.take() else { return };
        if matches!(state.child.try_wait(), Ok(None)) {
            let _ = state.child.start_kill();
            if tokio::time::timeout(STOP_GRACE, state.child.wait()).await.is_err() {
                tracing::warn!("llama-server did not exit within {STOP_GRACE:?}");
            }
        }
        self.forget_pid(slot);
    }

    fn stop_leftover_slot(&self, slot: &Slot) {
        let pid_file = self.data_dir.join(slot.pid_file);
        let Ok(raw) = std::fs::read_to_string(&pid_file) else { return };
        if let Ok(pid) = raw.trim().parse::<u32>() {
            if pid != slot.pid.load(Ordering::SeqCst) && kill_if_llama(pid) {
                tracing::info!("stopped a leftover llama-server (pid {pid})");
            }
        }
        let _ = std::fs::remove_file(pid_file);
    }

    fn kill_slot(&self, slot: &Slot) {
        let pid = slot.pid.swap(0, Ordering::SeqCst);
        if pid != 0 {
            kill_if_llama(pid);
            let _ = std::fs::remove_file(self.data_dir.join(slot.pid_file));
        }
    }

    fn forget_pid(&self, slot: &Slot) {
        slot.pid.store(0, Ordering::SeqCst);
        let _ = std::fs::remove_file(self.data_dir.join(slot.pid_file));
    }
}

fn free_port() -> Result<u16, LlmError> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| LlmError::Runtime(error.to_string()))?;
    Ok(listener.local_addr().map_err(|error| LlmError::Runtime(error.to_string()))?.port())
}

/// Kills `pid` only if it is a llama-server, so a recycled pid never hits an unrelated process.
fn kill_if_llama(pid: u32) -> bool {
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    match system.process(pid) {
        Some(process) if process.name().to_string_lossy().contains("llama-server") => process.kill(),
        _ => false,
    }
}

fn last_error(log_path: &Path) -> Option<String> {
    let log = std::fs::read_to_string(log_path).ok()?;
    log.lines()
        .rev()
        .find(|line| {
            let lower = line.to_lowercase();
            lower.contains("error") || lower.contains("failed")
        })
        .map(|line| line.trim().to_owned())
}
