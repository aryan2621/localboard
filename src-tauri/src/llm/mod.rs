pub mod anthropic;
pub mod hardware;
pub mod ollama;
pub mod openai_compat;
pub mod provider;
pub mod router;

use serde::{Deserialize, Serialize};

use crate::engine::mcp::is_loopback;
use anthropic::AnthropicProvider;
use ollama::OllamaProvider;
use openai_compat::OpenAiCompatProvider;
use provider::{LlmError, LlmProvider, Task};
use router::TaskRouting;

/// App setting holding the [`LlmConfig`]. The API key lives in the keychain under `llm.<kind>`.
pub const CONFIG_KEY: &str = "llm.provider";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Anthropic,
    Openai,
    Gemini,
    Ollama,
    Compat,
}

impl Kind {
    pub fn secret_ref(self) -> String {
        format!("llm.{}", format!("{self:?}").to_lowercase())
    }

    fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => anthropic::DEFAULT_BASE_URL,
            Self::Openai => "https://api.openai.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Self::Ollama => "http://localhost:11434/v1",
            Self::Compat => "",
        }
    }
}

/// Advanced per-model settings. `None` keeps the provider's or the call site's default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelParams {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    /// Context window in tokens. Only Ollama's native API honours it.
    #[serde(default)]
    pub num_ctx: Option<u32>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl ModelParams {
    pub fn temperature(&self, default: f32) -> f32 {
        self.temperature.unwrap_or(default)
    }

    pub fn timeout(&self, default: std::time::Duration) -> std::time::Duration {
        self.timeout_secs.map_or(default, |secs| std::time::Duration::from_secs(secs.clamp(10, 3600)))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmConfig {
    pub kind: Kind,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub embed_model: Option<String>,
    #[serde(default)]
    pub params: ModelParams,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self { kind: Kind::Ollama, base_url: Kind::Ollama.default_base_url().into(), model: String::new(), embed_model: None, params: ModelParams::default() }
    }
}

impl LlmConfig {
    fn base_url(&self) -> &str {
        let url = self.base_url.trim();
        if url.is_empty() { self.kind.default_base_url() } else { url }
    }
}

/// A snapshot of the provider settings, key and offline flag: a factory for providers.
#[derive(Clone)]
pub struct Llm {
    config: LlmConfig,
    api_key: Option<String>,
    offline: bool,
}

impl Llm {
    pub fn new(config: LlmConfig, api_key: Option<String>, offline: bool) -> Self {
        Self { config, api_key: api_key.filter(|key| !key.is_empty()), offline }
    }

    fn check_online(&self) -> Result<(), LlmError> {
        if self.offline && !is_loopback(self.config.base_url()) {
            return Err(LlmError::Invalid("Offline mode is on. Calling a remote AI provider would leave this device.".into()));
        }
        Ok(())
    }

    fn model<'a>(&'a self, model_override: Option<&'a str>) -> Option<&'a str> {
        model_override.or(Some(self.config.model.as_str())).map(str::trim).filter(|model| !model.is_empty())
    }

    fn build(&self, model: &str) -> Result<Box<dyn LlmProvider>, LlmError> {
        let base_url = self.config.base_url();
        if base_url.is_empty() {
            return Err(LlmError::Invalid("Set a base URL for the AI provider in Settings, AI.".into()));
        }
        let params = self.config.params.clone();
        Ok(match self.config.kind {
            Kind::Anthropic => Box::new(AnthropicProvider::new(base_url, self.api_key.clone(), model, params)?),
            Kind::Ollama => Box::new(OllamaProvider::new(base_url, model, params)?),
            _ => Box::new(OpenAiCompatProvider::new(base_url, self.api_key.clone(), model, params)?),
        })
    }

    /// `None` uses the configured model.
    pub fn provider(&self, model_override: Option<&str>) -> Result<Box<dyn LlmProvider>, LlmError> {
        self.check_online()?;
        self.build(self.model(model_override).ok_or(LlmError::NoModel)?)
    }

    pub fn task_provider(&self, routing: &TaskRouting, task: Task) -> Result<Box<dyn LlmProvider>, LlmError> {
        self.provider(routing.get(task))
    }

    /// `None` means keyword matching is used instead of vectors.
    pub fn embed_provider(&self) -> Option<Box<dyn LlmProvider>> {
        let model = self.config.embed_model.as_deref().map(str::trim).filter(|model| !model.is_empty())?;
        if self.config.kind == Kind::Anthropic || self.check_online().is_err() {
            return None;
        }
        self.build(model).ok()
    }

    /// False means SQL-only mode: nothing configured, or offline with a remote provider.
    pub fn available(&self, routing: &TaskRouting) -> bool {
        self.check_online().is_ok() && self.model(routing.get(Task::Sql)).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llm(kind: Kind, base_url: &str, model: &str, offline: bool) -> Llm {
        Llm::new(LlmConfig { kind, base_url: base_url.into(), model: model.into(), embed_model: None, params: ModelParams::default() }, None, offline)
    }

    #[test]
    fn default_is_unconfigured_ollama() {
        let config = LlmConfig::default();
        assert_eq!((config.kind, config.base_url.as_str(), config.model.as_str()), (Kind::Ollama, "http://localhost:11434/v1", ""));
        assert!(matches!(Llm::new(config, None, false).provider(None), Err(LlmError::NoModel)));
    }

    #[test]
    fn offline_allows_only_loopback() {
        let routing = TaskRouting::default();
        assert!(llm(Kind::Ollama, "", "llama3", true).available(&routing));
        assert!(llm(Kind::Compat, "http://127.0.0.1:8000/v1", "m", true).provider(None).is_ok());
        assert!(!llm(Kind::Openai, "", "gpt", true).available(&routing));
        assert!(llm(Kind::Openai, "", "gpt", true).provider(None).is_err());
        assert!(llm(Kind::Openai, "", "gpt", false).provider(None).is_ok());
    }

    #[test]
    fn routing_overrides_the_configured_model() {
        let llm = llm(Kind::Openai, "", "default", false);
        assert_eq!(llm.provider(Some("other")).unwrap().id(), "other");
        assert_eq!(llm.provider(None).unwrap().id(), "default");
    }

    #[test]
    fn secret_refs_and_kinds() {
        assert_eq!(Kind::Openai.secret_ref(), "llm.openai");
        assert_eq!(serde_json::to_string(&Kind::Anthropic).unwrap(), "\"anthropic\"");
    }
}
