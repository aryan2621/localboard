use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::catalog::ModelSpec;
use super::provider::LlmError;

pub const HUGGING_FACE: &str = "https://huggingface.co";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
    pub finished: bool,
}

impl DownloadProgress {
    fn active(&self) -> bool {
        !self.finished && self.error.is_none()
    }
}

#[derive(Deserialize)]
struct RevisionInfo {
    sha: String,
    siblings: Vec<Sibling>,
}

#[derive(Deserialize)]
struct Sibling {
    rfilename: String,
    size: Option<u64>,
    lfs: Option<LfsInfo>,
}

#[derive(Deserialize)]
struct LfsInfo {
    sha256: String,
    size: u64,
}

pub type ProgressSink = Arc<dyn Fn(&DownloadProgress) + Send + Sync>;

/// Downloads one model at a time: pins the repository revision, streams into a `.part` file
/// while hashing, and only renames into place when size and SHA-256 match the published LFS digest.
pub struct Downloader {
    client: reqwest::Client,
    hub: String,
    state: Mutex<Option<DownloadProgress>>,
    cancel: AtomicBool,
}

impl Downloader {
    pub fn new(hub: impl Into<String>) -> Result<Self, LlmError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self { client, hub: hub.into(), state: Mutex::new(None), cancel: AtomicBool::new(false) })
    }

    pub fn status(&self) -> Option<DownloadProgress> {
        self.state.lock().ok().and_then(|state| state.clone())
    }

    pub fn is_downloading(&self, id: &str) -> bool {
        self.status().is_some_and(|progress| progress.id == id && progress.active())
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Starts a background download; progress (and the final state) goes to `sink`.
    pub fn start(self: &Arc<Self>, spec: &'static ModelSpec, target: PathBuf, sink: ProgressSink) -> Result<(), LlmError> {
        {
            let mut state = self.state.lock().map_err(|_| LlmError::Download("Download state is unavailable".into()))?;
            if state.as_ref().is_some_and(DownloadProgress::active) {
                return Err(LlmError::Download("Another model is downloading".into()));
            }
            *state = Some(DownloadProgress { id: spec.id.into(), done: 0, total: 0, error: None, finished: false });
        }
        self.cancel.store(false, Ordering::SeqCst);
        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = this.fetch(spec, &target, &sink).await;
            let final_state = this.update(|progress| match &result {
                Ok(()) => progress.finished = true,
                Err(error) => progress.error = Some(error.to_string()),
            });
            if let Err(error) = &result {
                tracing::warn!("model download {} failed: {error}", spec.id);
            }
            if let Some(progress) = final_state {
                sink(&progress);
            }
        });
        Ok(())
    }

    async fn fetch(&self, spec: &ModelSpec, target: &Path, sink: &ProgressSink) -> Result<(), LlmError> {
        let part = target.with_extension("gguf.part");
        let result = self.fetch_into(spec, &part, sink).await;
        match result {
            Ok(()) => tokio::fs::rename(&part, target)
                .await
                .map_err(|error| LlmError::Download(format!("Could not save the model: {error}"))),
            Err(error) => {
                let _ = tokio::fs::remove_file(&part).await;
                Err(error)
            }
        }
    }

    async fn fetch_into(&self, spec: &ModelSpec, part: &Path, sink: &ProgressSink) -> Result<(), LlmError> {
        let info: RevisionInfo = self
            .client
            .get(format!("{}/api/models/{}/revision/main", self.hub, spec.repo))
            .query(&[("blobs", "true")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let sibling = info
            .siblings
            .iter()
            .find(|sibling| sibling.rfilename == spec.file)
            .ok_or_else(|| LlmError::Download(format!("{} is not published in {}", spec.file, spec.repo)))?;
        let lfs = sibling
            .lfs
            .as_ref()
            .ok_or_else(|| LlmError::Download("The model has no published checksum; refusing to download".into()))?;
        let total = sibling.size.unwrap_or(lfs.size);
        let progress = self.update(|progress| progress.total = total);
        if let Some(progress) = progress {
            sink(&progress);
        }

        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| LlmError::Download(error.to_string()))?;
        }
        let mut file = tokio::fs::File::create(part).await.map_err(|error| LlmError::Download(error.to_string()))?;
        let response = self
            .client
            .get(format!("{}/{}/resolve/{}/{}", self.hub, spec.repo, info.sha, spec.file))
            .send()
            .await?
            .error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        let mut last_report = Instant::now();
        while let Some(chunk) = stream.next().await {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(LlmError::Download("Download cancelled".into()));
            }
            let chunk = chunk?;
            file.write_all(&chunk).await.map_err(|error| LlmError::Download(error.to_string()))?;
            hasher.update(&chunk);
            done += chunk.len() as u64;
            if done > total {
                return Err(LlmError::Download("The download is larger than published; aborting".into()));
            }
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = Instant::now();
                if let Some(progress) = self.update(|progress| progress.done = done) {
                    sink(&progress);
                }
            }
        }
        file.flush().await.map_err(|error| LlmError::Download(error.to_string()))?;
        file.sync_all().await.map_err(|error| LlmError::Download(error.to_string()))?;
        self.update(|progress| progress.done = done);
        let digest = format!("{:x}", hasher.finalize());
        if done != total || !digest.eq_ignore_ascii_case(&lfs.sha256) {
            return Err(LlmError::Download("The download was damaged (checksum mismatch). Please try again.".into()));
        }
        Ok(())
    }

    fn update(&self, change: impl FnOnce(&mut DownloadProgress)) -> Option<DownloadProgress> {
        let mut state = self.state.lock().ok()?;
        let progress = state.as_mut()?;
        change(progress);
        Some(progress.clone())
    }
}
