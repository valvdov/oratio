use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{prompt, PolishProvider, PolishRequest};
use crate::{Error, Result};

/// One OpenAI-compatible chat-completions provider. Covers Ollama
/// (`http://127.0.0.1:11434/v1`), OpenRouter, Gemini's compat endpoint,
/// LM Studio — anything speaking the same protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Ollama-specific: how long to keep the model in RAM after a request.
    /// Ignored by cloud providers. E.g. "2h" or "-1" (forever).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

pub struct OpenAiCompat {
    config: ProviderConfig,
    client: reqwest::blocking::Client,
}

impl OpenAiCompat {
    pub fn new(config: ProviderConfig, timeout_ms: u64) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .expect("http client");
        Self { config, client }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<&'a str>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

impl PolishProvider for OpenAiCompat {
    fn name(&self) -> &str {
        &self.config.id
    }

    fn polish(&self, req: &PolishRequest) -> Result<String> {
        let system = prompt::build_system_prompt(req);
        let body = ChatRequest {
            model: &self.config.model,
            messages: vec![
                Message {
                    role: "system",
                    content: &system,
                },
                Message {
                    role: "user",
                    content: req.raw,
                },
            ],
            temperature: 0.2,
            stream: false,
            keep_alive: self.config.keep_alive.as_deref(),
        };

        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        let mut http = self.client.post(&url).json(&body);
        if let Some(key) = self.config.api_key.as_deref() {
            http = http.bearer_auth(key);
        }
        let resp = http.send().map_err(|e| Error::Polish(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(Error::Polish(format!(
                "{url} returned {status}: {}",
                text.chars().take(300).collect::<String>()
            )));
        }
        let parsed: ChatResponse = resp.json().map_err(|e| Error::Polish(e.to_string()))?;
        let content = parsed
            .choices
            .first()
            .map(|c| c.message.content.trim().to_string())
            .unwrap_or_default();
        if content.is_empty() {
            return Err(Error::Polish("empty completion".into()));
        }
        Ok(strip_think(&content))
    }
}

/// Some local models (qwen3 hybrid modes) may emit <think>...</think> blocks.
fn strip_think(text: &str) -> String {
    match (text.find("<think>"), text.find("</think>")) {
        (Some(start), Some(end)) if end > start => {
            let mut out = String::new();
            out.push_str(&text[..start]);
            out.push_str(&text[end + "</think>".len()..]);
            out.trim().to_string()
        }
        _ => text.trim().to_string(),
    }
}

/// Quick connectivity probe for the settings UI "Test" button.
pub fn test_provider(config: &ProviderConfig, timeout_ms: u64) -> Result<String> {
    let provider = OpenAiCompat::new(config.clone(), timeout_ms);
    let req = PolishRequest {
        raw: "ну эээ проверка связи так сказать",
        style: None,
        dictionary: &[],
    };
    provider.polish(&req)
}

/// Fire a trivial completion to page the model into RAM (local providers take
/// 10-15s to load from disk). Call from a background thread at app start.
pub fn warm_up(config: &ProviderConfig) -> Result<()> {
    let provider = OpenAiCompat::new(config.clone(), 120_000);
    let req = PolishRequest {
        raw: "ок",
        style: None,
        dictionary: &[],
    };
    provider.polish(&req).map(|_| ())
}
