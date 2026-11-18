use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::provider::{ChatRequest, ChatResponse, LlmError, LlmProvider, ResponseFormat, Usage};
use super::ModelParams;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Ollama's native API. The OpenAI-compatible `/v1` endpoint ignores `num_ctx`, so context size needs this one.
pub struct OllamaProvider {
    client: reqwest::Client,
    root: String,
    model: String,
    params: ModelParams,
}

#[derive(Deserialize, Default)]
struct Reply {
    message: Option<Message>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
    /// Nanoseconds spent generating.
    eval_duration: Option<u64>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

impl Reply {
    fn finish(self, content: String) -> Result<ChatResponse, LlmError> {
        if let Some(error) = self.error {
            return Err(LlmError::Request(error));
        }
        let (tokens, nanos) = (self.eval_count.unwrap_or(0), self.eval_duration.unwrap_or(0));
        Ok(ChatResponse {
            content,
            tokens_per_sec: (nanos > 0).then(|| f64::from(tokens) / (nanos as f64 / 1e9)),
            usage: Usage { prompt_tokens: self.prompt_eval_count.unwrap_or(0), completion_tokens: tokens },
        })
    }
}

impl OllamaProvider {
    /// `base_url` is the configured `.../v1` URL; the native API lives at the server root.
    pub fn new(base_url: &str, model: impl Into<String>, params: ModelParams) -> Result<Self, LlmError> {
        let root = base_url.trim_end_matches('/').trim_end_matches("/v1").to_owned();
        Ok(Self { client: reqwest::Client::builder().timeout(params.timeout(REQUEST_TIMEOUT)).build()?, root, model: model.into(), params })
    }

    fn body(&self, request: &ChatRequest, stream: bool) -> Value {
        let mut options = json!({
            "temperature": self.params.temperature(request.temperature),
            "num_predict": request.max_tokens,
        });
        if let Some(top_p) = self.params.top_p {
            options["top_p"] = json!(top_p);
        }
        if let Some(num_ctx) = self.params.num_ctx {
            options["num_ctx"] = json!(num_ctx);
        }
        let mut body = json!({ "model": self.model, "messages": request.messages, "stream": stream, "options": options });
        if let ResponseFormat::JsonSchema { schema, .. } = &request.format {
            body["format"] = schema.clone();
        }
        body
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn id(&self) -> &str {
        &self.model
    }

    async fn health(&self) -> Result<(), LlmError> {
        self.client.get(format!("{}/api/tags", self.root)).send().await?.error_for_status()?;
        Ok(())
    }

    async fn chat(&self, request: ChatRequest, tokens: Option<mpsc::Sender<String>>) -> Result<ChatResponse, LlmError> {
        let response = self.client.post(format!("{}/api/chat", self.root)).json(&self.body(&request, tokens.is_some())).send().await?.error_for_status()?;
        let Some(tokens) = tokens else {
            let reply: Reply = response.json().await?;
            let content = reply.message.as_ref().and_then(|message| message.content.clone()).unwrap_or_default();
            return reply.finish(content);
        };

        // Streaming is newline-delimited JSON; the last object carries the counts.
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut content = String::new();
        let mut last = Reply::default();
        while let Some(chunk) = stream.next().await {
            buffer.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(end) = buffer.find('\n') {
                let line = buffer[..end].trim().to_owned();
                buffer.drain(..=end);
                if line.is_empty() {
                    continue;
                }
                let event: Reply = serde_json::from_str(&line).map_err(|error| LlmError::Request(format!("malformed stream event: {error}")))?;
                if let Some(delta) = event.message.as_ref().and_then(|message| message.content.clone()).filter(|delta| !delta.is_empty()) {
                    if tokens.send(delta.clone()).await.is_err() {
                        return Err(LlmError::Request("the response stream was closed by the receiver".into()));
                    }
                    content.push_str(&delta);
                }
                last = event;
            }
        }
        last.finish(content)
    }

    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        let body: Value = self
            .client
            .post(format!("{}/api/embed", self.root))
            .json(&json!({ "model": self.model, "input": inputs }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let vectors: Vec<Vec<f32>> = serde_json::from_value(body["embeddings"].clone()).map_err(|_| LlmError::Request("the server returned no embeddings".into()))?;
        if vectors.len() != inputs.len() {
            return Err(LlmError::Request("the server returned the wrong number of embeddings".into()));
        }
        Ok(vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{ChatMessage, Role};

    #[test]
    fn native_body_carries_context_size_and_strips_v1() {
        let params = ModelParams { num_ctx: Some(16384), top_p: Some(0.9), ..Default::default() };
        let provider = OllamaProvider::new("http://localhost:11434/v1/", "m", params).unwrap();
        assert_eq!(provider.root, "http://localhost:11434");
        let request = ChatRequest {
            messages: vec![ChatMessage { role: Role::User, content: "hi".into() }],
            temperature: 0.1,
            max_tokens: 50,
            format: ResponseFormat::Text,
        };
        let body = provider.body(&request, false);
        assert_eq!(body["options"]["num_ctx"], 16384);
        assert_eq!(body["options"]["num_predict"], 50);
        assert!(body.get("format").is_none());
    }
}
