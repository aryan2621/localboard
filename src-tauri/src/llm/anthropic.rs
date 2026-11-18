//! Native adapter for Anthropic's Messages API (`/v1/messages`).

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::ModelParams;
use super::provider::{ChatRequest, ChatResponse, LlmError, LlmProvider, ResponseFormat, Role, Usage};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const API_VERSION: &str = "2023-06-01";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    params: ModelParams,
}

/// Structured output is a single forced tool call; tool names allow only `[a-zA-Z0-9_-]`, max 64.
fn tool_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).take(64).collect();
    if clean.is_empty() { "respond".into() } else { clean }
}

fn build_body(model: &str, request: &ChatRequest, stream: bool) -> Value {
    let system = request.messages.iter().filter(|m| matches!(m.role, Role::System)).map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n\n");
    let messages: Vec<Value> = request
        .messages
        .iter()
        .filter(|m| !matches!(m.role, Role::System))
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();
    let mut body = json!({
        "model": model,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "messages": messages,
        "stream": stream,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if let ResponseFormat::JsonSchema { name, schema } = &request.format {
        let name = tool_name(name);
        body["tools"] = json!([{ "name": name, "description": "Return the answer as this structured object.", "input_schema": schema }]);
        body["tool_choice"] = json!({ "type": "tool", "name": name });
    }
    body
}

fn usage_of(value: &Value, usage: &mut Usage) {
    if let Some(n) = value["input_tokens"].as_u64() {
        usage.prompt_tokens = n as u32;
    }
    if let Some(n) = value["output_tokens"].as_u64() {
        usage.completion_tokens = n as u32;
    }
}

/// Text blocks, or the forced tool call's input serialized as JSON.
fn parse_response(value: &Value) -> ChatResponse {
    let content = value["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| match block["type"].as_str() {
                    Some("text") => block["text"].as_str().map(str::to_owned),
                    Some("tool_use") => Some(block["input"].to_string()),
                    _ => None,
                })
                .collect::<String>()
        })
        .unwrap_or_default();
    let mut usage = Usage::default();
    usage_of(&value["usage"], &mut usage);
    ChatResponse { content, tokens_per_sec: None, usage }
}

/// Folds one SSE `data:` payload into `result`; returns the content delta, if any.
fn apply_stream_event(data: &str, result: &mut ChatResponse) -> Result<Option<String>, LlmError> {
    let event: Value = serde_json::from_str(data).map_err(|error| LlmError::Request(format!("malformed stream event: {error}")))?;
    match event["type"].as_str() {
        Some("content_block_delta") => {
            let delta = &event["delta"];
            let text = delta["text"].as_str().or_else(|| delta["partial_json"].as_str());
            if let Some(text) = text {
                result.content.push_str(text);
                return Ok(Some(text.to_owned()));
            }
        }
        Some("message_start") => usage_of(&event["message"]["usage"], &mut result.usage),
        Some("message_delta") => usage_of(&event["usage"], &mut result.usage),
        Some("error") => return Err(LlmError::Request(event["error"]["message"].as_str().unwrap_or("stream error").to_owned())),
        _ => {}
    }
    Ok(None)
}

impl AnthropicProvider {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>, model: impl Into<String>, params: ModelParams) -> Result<Self, LlmError> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(params.timeout(REQUEST_TIMEOUT)).build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            model: model.into(),
            params,
        })
    }

    fn headers(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = request.header("anthropic-version", API_VERSION);
        match &self.api_key {
            Some(key) => request.header("x-api-key", key),
            None => request,
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.model
    }

    async fn health(&self) -> Result<(), LlmError> {
        self.headers(self.client.get(format!("{}/v1/models", self.base_url))).send().await?.error_for_status()?;
        Ok(())
    }

    async fn chat(&self, request: ChatRequest, tokens: Option<mpsc::Sender<String>>) -> Result<ChatResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url);
        let stream = tokens.is_some();
        let response = self.headers(self.client.post(url)).json(&build_body(&self.model, &ChatRequest { temperature: self.params.temperature(request.temperature), ..request.clone() }, stream)).send().await?.error_for_status()?;
        let Some(tokens) = tokens else { return Ok(parse_response(&response.json().await?)) };

        let mut bytes = response.bytes_stream();
        let mut buffer = String::new();
        let mut result = ChatResponse { content: String::new(), tokens_per_sec: None, usage: Usage::default() };
        while let Some(chunk) = bytes.next().await {
            buffer.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(end) = buffer.find('\n') {
                let line = buffer[..end].trim().to_owned();
                buffer.drain(..=end);
                let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
                if let Some(delta) = apply_stream_event(data, &mut result)? {
                    if tokens.send(delta).await.is_err() {
                        return Err(LlmError::Request("the response stream was closed by the receiver".into()));
                    }
                }
            }
        }
        Ok(result)
    }

    async fn embed(&self, _inputs: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        Err(LlmError::Unsupported("Anthropic has no embeddings API; keyword search is used instead.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ChatMessage;

    fn request(format: ResponseFormat) -> ChatRequest {
        ChatRequest {
            messages: vec![
                ChatMessage { role: Role::System, content: "be brief".into() },
                ChatMessage { role: Role::User, content: "hi".into() },
            ],
            temperature: 0.0,
            max_tokens: 100,
            format,
        }
    }

    #[test]
    fn system_is_sent_separately() {
        let body = build_body("m", &request(ResponseFormat::Text), false);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn schema_becomes_forced_tool() {
        let schema = json!({ "type": "object" });
        let body = build_body("m", &request(ResponseFormat::JsonSchema { name: "my plan!".into(), schema: schema.clone() }), true);
        assert_eq!(body["tools"][0]["name"], "my_plan_");
        assert_eq!(body["tools"][0]["input_schema"], schema);
        assert_eq!(body["tool_choice"], json!({ "type": "tool", "name": "my_plan_" }));
    }

    #[test]
    fn parses_text_and_tool_input() {
        let text = parse_response(&json!({ "content": [{ "type": "text", "text": "ok" }], "usage": { "input_tokens": 3, "output_tokens": 2 } }));
        assert_eq!((text.content.as_str(), text.usage.prompt_tokens, text.usage.completion_tokens), ("ok", 3, 2));
        let tool = parse_response(&json!({ "content": [{ "type": "tool_use", "input": { "a": 1 } }] }));
        assert_eq!(tool.content, r#"{"a":1}"#);
    }

    #[test]
    fn folds_stream_events() {
        let mut result = ChatResponse { content: String::new(), tokens_per_sec: None, usage: Usage::default() };
        apply_stream_event(r#"{"type":"message_start","message":{"usage":{"input_tokens":7,"output_tokens":1}}}"#, &mut result).unwrap();
        let a = apply_stream_event(r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"he"}}"#, &mut result).unwrap();
        let b = apply_stream_event(r#"{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"llo"}}"#, &mut result).unwrap();
        apply_stream_event(r#"{"type":"message_delta","usage":{"output_tokens":9}}"#, &mut result).unwrap();
        assert_eq!((a.as_deref(), b.as_deref()), (Some("he"), Some("llo")));
        assert_eq!((result.content.as_str(), result.usage.prompt_tokens, result.usage.completion_tokens), ("hello", 7, 9));
        assert!(apply_stream_event(r#"{"type":"error","error":{"message":"overloaded"}}"#, &mut result).is_err());
    }
}
