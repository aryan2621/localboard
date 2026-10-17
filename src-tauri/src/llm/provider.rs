use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("No AI model is downloaded yet. Download one in Settings, Models.")]
    NoModel,
    #[error("{0}")]
    Invalid(String),
    #[error("The AI runtime failed: {0}")]
    Runtime(String),
    #[error("The AI request failed: {0}")]
    Request(String),
    #[error("{0}")]
    Download(String),
}

impl From<reqwest::Error> for LlmError {
    fn from(error: reqwest::Error) -> Self {
        Self::Request(error.to_string())
    }
}

/// What a model call is for; each task can be routed to a different model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Task {
    Sql,
    Reasoning,
    Narrative,
    Embeddings,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Debug)]
pub enum ResponseFormat {
    Text,
    /// Grammar-constrained decoding against a JSON Schema.
    JsonSchema { name: String, schema: Value },
}

#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub format: ResponseFormat,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub tokens_per_sec: Option<f64>,
    pub usage: Usage,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    #[expect(dead_code, reason = "identifies custom OpenAI-compatible endpoints configured in Settings")]
    fn id(&self) -> &str;
    #[expect(dead_code, reason = "custom OpenAI-compatible endpoints are health-checked from Settings")]
    async fn health(&self) -> Result<(), LlmError>;
    /// When `tokens` is set, content deltas are streamed into it as they arrive.
    async fn chat(&self, request: ChatRequest, tokens: Option<mpsc::Sender<String>>) -> Result<ChatResponse, LlmError>;
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, LlmError>;
}
