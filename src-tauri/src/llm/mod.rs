pub mod catalog;
pub mod download;
pub mod hardware;
pub mod llama_server;
pub mod openai_compat;
pub mod provider;
pub mod router;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use catalog::{ModelSpec, ModelStore, CATALOG};
use download::{DownloadProgress, Downloader, ProgressSink};
use hardware::Hardware;
use llama_server::Supervisor;
use openai_compat::OpenAiCompatProvider;
use provider::{ChatMessage, ChatRequest, LlmError, LlmProvider, ResponseFormat, Role, Task};
use router::TaskRouting;

const BENCHMARK_PROMPT: &str = "In three sentences, explain why a bar chart suits comparing sales across regions.";
const BENCHMARK_TOKENS: u32 = 128;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    #[serde(flatten)]
    pub spec: &'static ModelSpec,
    pub installed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub ram_gb: u64,
    pub recommended: &'static str,
    pub runtime_available: bool,
    pub running: Option<String>,
    pub download: Option<DownloadProgress>,
    pub routing: TaskRouting,
    /// What each task resolves to right now; `None` means SQL-only mode.
    pub active: Option<&'static str>,
    pub models: Vec<ModelView>,
    pub embedding: ModelView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Benchmark {
    pub model_id: String,
    pub load_ms: u128,
    pub tokens_per_sec: f64,
    pub completion_tokens: u32,
}

/// The local AI runtime: model files, downloads and the llama.cpp server process.
pub struct LlmRuntime {
    store: ModelStore,
    downloader: Arc<Downloader>,
    supervisor: Supervisor,
    hardware: Hardware,
}

impl LlmRuntime {
    pub fn new(data_dir: PathBuf, binary: Option<PathBuf>) -> Result<Self, LlmError> {
        let supervisor = Supervisor::new(data_dir.clone(), binary)?;
        Ok(Self {
            store: ModelStore::new(data_dir.join("models")),
            downloader: Arc::new(Downloader::new(download::HUGGING_FACE)?),
            supervisor,
            hardware: hardware::detect(),
        })
    }

    /// Kills llama-server processes left behind by a force-quit. Not part of construction:
    /// scanning processes on the UI thread keeps the window from painting.
    pub fn stop_leftover_servers(&self) {
        self.supervisor.stop_leftover();
    }

    pub fn hardware(&self) -> &Hardware {
        &self.hardware
    }

    pub fn resolve(&self, routing: &TaskRouting, task: Task) -> Option<&'static ModelSpec> {
        self.store.resolve(routing.get(task), self.hardware.ram_gb)
    }

    pub fn runtime_available(&self) -> bool {
        self.supervisor.binary_available()
    }

    pub async fn running_model(&self) -> Option<String> {
        self.supervisor.current().await.map(|endpoint| endpoint.model_id)
    }

    pub async fn catalog(&self, routing: TaskRouting) -> CatalogView {
        CatalogView {
            ram_gb: self.hardware.ram_gb,
            recommended: catalog::recommended(self.hardware.ram_gb).id,
            runtime_available: self.runtime_available(),
            running: self.running_model().await,
            download: self.downloader.status(),
            active: self.resolve(&routing, Task::Sql).map(|spec| spec.id),
            routing,
            models: CATALOG
                .iter()
                .map(|spec| ModelView { spec, installed: self.store.path(spec.id).is_some() })
                .collect(),
            embedding: ModelView { spec: &catalog::EMBEDDING, installed: self.store.path(catalog::EMBEDDING.id).is_some() },
        }
    }

    pub fn download(&self, model_id: &str, sink: ProgressSink) -> Result<(), LlmError> {
        let spec = known(model_id)?;
        if self.store.path(spec.id).is_some() {
            return Err(LlmError::Invalid(format!("{} is already installed", spec.name)));
        }
        self.downloader.start(spec, self.store.own_path(spec.id), sink)
    }

    pub fn cancel_download(&self) {
        self.downloader.cancel();
    }

    pub async fn delete(&self, model_id: &str) -> Result<(), LlmError> {
        let spec = known(model_id)?;
        if self.downloader.is_downloading(spec.id) {
            return Err(LlmError::Invalid("Cancel the download first".into()));
        }
        if spec.id == catalog::EMBEDDING.id {
            if self.supervisor.embed_model().await.as_deref() == Some(spec.id) {
                self.supervisor.stop_embed().await;
            }
        } else if self.supervisor.current().await.is_some_and(|endpoint| endpoint.model_id == spec.id) {
            self.supervisor.stop_chat().await;
        }
        match std::fs::remove_file(self.store.own_path(spec.id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(LlmError::Runtime(format!("could not delete the model: {error}"))),
        }
    }

    /// Starts the embedding model when it is installed. `Ok(None)` means keyword matching is used.
    pub async fn embed_provider(&self) -> Result<Option<OpenAiCompatProvider>, LlmError> {
        if !self.runtime_available() {
            return Ok(None);
        }
        let Some(path) = self.store.path(catalog::EMBEDDING.id) else { return Ok(None) };
        let endpoint = self.supervisor.ensure_embed(catalog::EMBEDDING.id, &path).await?;
        Ok(Some(OpenAiCompatProvider::new(endpoint.base_url, Some(endpoint.api_key), endpoint.model_id)?))
    }

    /// Loads the model into llama-server and returns a provider bound to it.
    pub async fn provider(&self, model_id: &str) -> Result<OpenAiCompatProvider, LlmError> {
        let spec = known(model_id)?;
        let path = self.store.path(spec.id).ok_or(LlmError::NoModel)?;
        let endpoint = self.supervisor.ensure(spec.id, &path).await?;
        OpenAiCompatProvider::new(endpoint.base_url, Some(endpoint.api_key), endpoint.model_id)
    }

    pub async fn stop(&self) {
        self.supervisor.stop().await;
    }

    pub fn shutdown(&self) {
        self.downloader.cancel();
        self.supervisor.kill_now();
    }

    pub async fn benchmark(&self, model_id: &str) -> Result<Benchmark, LlmError> {
        let loading = Instant::now();
        let provider = self.provider(model_id).await?;
        let load_ms = loading.elapsed().as_millis();
        let generating = Instant::now();
        let response = provider
            .chat(
                ChatRequest {
                    messages: vec![ChatMessage { role: Role::User, content: BENCHMARK_PROMPT.into() }],
                    temperature: 0.0,
                    max_tokens: BENCHMARK_TOKENS,
                    format: ResponseFormat::Text,
                },
                None,
            )
            .await?;
        let elapsed = generating.elapsed().as_secs_f64();
        let measured = (elapsed > 0.0).then(|| f64::from(response.usage.completion_tokens) / elapsed);
        Ok(Benchmark {
            model_id: model_id.to_owned(),
            load_ms,
            tokens_per_sec: response.tokens_per_sec.or(measured).unwrap_or(0.0),
            completion_tokens: response.usage.completion_tokens,
        })
    }
}

fn known(model_id: &str) -> Result<&'static ModelSpec, LlmError> {
    catalog::find(model_id).ok_or_else(|| LlmError::Invalid(format!("Unknown model: {model_id}")))
}
