use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::provider::{ChatRequest, ChatResponse, LlmError, LlmProvider, ResponseFormat, Usage};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Any server speaking the OpenAI chat API: the bundled llama-server, LM Studio, vLLM.
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
    usage: Option<UsageBody>,
    timings: Option<Timings>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<MessageBody>,
    delta: Option<MessageBody>,
}

#[derive(Deserialize)]
struct MessageBody {
    content: Option<String>,
}

#[derive(Deserialize)]
struct UsageBody {
    prompt_tokens: u32,
    completion_tokens: u32,
}

/// llama.cpp extension: generation speed measured by the server itself.
#[derive(Deserialize)]
struct Timings {
    predicted_per_second: Option<f64>,
}

#[derive(Deserialize)]
struct Embeddings {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

impl OpenAiCompatProvider {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>, model: impl Into<String>) -> Result<Self, LlmError> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            model: model.into(),
        })
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let request = self.client.post(format!("{}{path}", self.base_url));
        match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }

    fn body(&self, request: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": request.messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens,
            "stream": stream,
        });
        if stream {
            body["stream_options"] = json!({ "include_usage": true });
        }
        if let ResponseFormat::JsonSchema { name, schema } = &request.format {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": name, "schema": schema, "strict": true }
            });
        }
        body
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn id(&self) -> &str {
        &self.model
    }

    async fn health(&self) -> Result<(), LlmError> {
        let mut request = self.client.get(format!("{}/models", self.base_url));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        request.send().await?.error_for_status()?;
        Ok(())
    }

    async fn chat(&self, request: ChatRequest, tokens: Option<mpsc::Sender<String>>) -> Result<ChatResponse, LlmError> {
        let Some(tokens) = tokens else {
            let completion: Completion =
                self.post("/chat/completions").json(&self.body(&request, false)).send().await?.error_for_status()?.json().await?;
            let content = completion
                .choices
                .into_iter()
                .next()
                .and_then(|choice| choice.message)
                .and_then(|message| message.content)
                .unwrap_or_default();
            return Ok(ChatResponse {
                content,
                tokens_per_sec: completion.timings.and_then(|timings| timings.predicted_per_second),
                usage: completion.usage.map(Usage::from).unwrap_or_default(),
            });
        };

        let response = self.post("/chat/completions").json(&self.body(&request, true)).send().await?.error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut result = ChatResponse { content: String::new(), tokens_per_sec: None, usage: Usage::default() };
        while let Some(chunk) = stream.next().await {
            buffer.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(end) = buffer.find('\n') {
                let line = buffer[..end].trim().to_owned();
                buffer.drain(..=end);
                let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
                if data == "[DONE]" {
                    return Ok(result);
                }
                let event: Completion =
                    serde_json::from_str(data).map_err(|error| LlmError::Request(format!("malformed stream event: {error}")))?;
                if let Some(delta) = event.choices.into_iter().next().and_then(|choice| choice.delta).and_then(|delta| delta.content) {
                    if tokens.send(delta.clone()).await.is_err() {
                        return Err(LlmError::Request("the response stream was closed by the receiver".into()));
                    }
                    result.content.push_str(&delta);
                }
                if let Some(usage) = event.usage {
                    result.usage = usage.into();
                }
                if let Some(speed) = event.timings.and_then(|timings| timings.predicted_per_second) {
                    result.tokens_per_sec = Some(speed);
                }
            }
        }
        Ok(result)
    }

    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        let embeddings: Embeddings = self
            .post("/embeddings")
            .json(&json!({ "model": self.model, "input": inputs }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if embeddings.data.len() != inputs.len() {
            return Err(LlmError::Request("the server returned the wrong number of embeddings".into()));
        }
        Ok(embeddings.data.into_iter().map(|item| item.embedding).collect())
    }
}

impl From<UsageBody> for Usage {
    fn from(usage: UsageBody) -> Self {
        Self { prompt_tokens: usage.prompt_tokens, completion_tokens: usage.completion_tokens }
    }
}
