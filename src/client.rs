use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

pub const SYSTEM_PROMPT: &str = "You are an expert systems engineer and software architect analyzing code diffs.\nProduce an architectural analysis strictly adhering to the following 4-Pillar schema:\n\n1. THE DATA JOURNEY: Step-by-step trace of how data enters, mutates, and exits the changed subsystem.\n2. ARCHITECTURAL PATTERN & DESIGN INTENT: Explicit identification of patterns applied (Event Bus, State Machine, ECS, Guard Clause, etc.).\n3. LANGUAGE & FRAMEWORK CAVEATS: Ecosystem hazards (Unity C# GC/hot-paths, Unreal C++ UPROPERTY ownership, Rust borrow bounds, Python GIL).\n4. CRITICAL ANCHORS & UNHANDLED EDGE CASES: Bounds errors, unhandled exceptions, dropped guard clauses, or silent failures.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_usage: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub index: Option<usize>,
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
}

#[derive(Debug, Clone)]
pub struct OllamaClient {
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
    client: reqwest::Client,
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl OllamaClient {
    pub fn new(endpoint: Option<String>, model: Option<String>) -> Self {
        let endpoint = endpoint
            .unwrap_or_else(|| "http:\x2F\x2Flocalhost:11434".to_string())
            .trim_end_matches('/')
            .to_string();
        let model = model.unwrap_or_else(|| "bench-reason-4b".to_string());
        Self {
            endpoint,
            model,
            temperature: 0.2,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_client(
        endpoint: Option<String>,
        model: Option<String>,
        client: reqwest::Client,
    ) -> Self {
        let endpoint = endpoint
            .unwrap_or_else(|| "http:\x2F\x2Flocalhost:11434".to_string())
            .trim_end_matches('/')
            .to_string();
        let model = model.unwrap_or_else(|| "bench-reason-4b".to_string());
        Self {
            endpoint,
            model,
            temperature: 0.2,
            client,
        }
    }

    pub fn build_request(&self, user_content: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage::system(SYSTEM_PROMPT),
                ChatMessage::user(user_content),
            ],
            temperature: self.temperature,
            stream: true,
            stream_options: None,
        }
    }

    pub async fn check_health(&self) -> Result<()> {
        let url = format!("{}/api/tags", self.endpoint);
        let resp = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(_) => {
                let root_url = format!("{}/", self.endpoint);
                let root_resp = self
                    .client
                    .get(&root_url)
                    .timeout(std::time::Duration::from_secs(3))
                    .send()
                    .await;
                if let Ok(r) = root_resp {
                    if r.status().is_success() {
                        return Ok(());
                    }
                }
                Err(anyhow!(
                    "Ollama daemon unreachable at {}. Start the service with 'ollama serve' or launch the Ollama application.",
                    self.endpoint
                ))
            }
            Err(_) => Err(anyhow!(
                "Ollama daemon unreachable at {}. Start the service with 'ollama serve' or launch the Ollama application.",
                self.endpoint
            )),
        }
    }

    pub async fn analyze_stream<F>(&self, payload: &str, mut on_chunk: F) -> Result<String>
    where
        F: FnMut(&str),
    {
        let url = format!("{}/v1/chat/completions", self.endpoint);
        let request_body = self.build_request(payload);

        let resp = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                anyhow!(
                    "Ollama daemon unreachable at {}. Start the service with 'ollama serve' or launch the Ollama application. Error: {}",
                    self.endpoint,
                    e
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama API returned error {}: {}", status, body));
        }

        let mut stream = resp.bytes_stream();
        let mut full_text = String::new();
        let mut buffer = String::new();

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.context("Failed to read SSE chunk from Ollama response stream")?;
            let text = std::str::from_utf8(&chunk)
                .context("SSE chunk received from Ollama is not valid UTF-8")?;
            buffer.push_str(text);

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if let Some(token) = parse_sse_line(&line) {
                    on_chunk(&token);
                    full_text.push_str(&token);
                }
            }
        }

        if !buffer.trim().is_empty() {
            if let Some(token) = parse_sse_line(&buffer) {
                on_chunk(&token);
                full_text.push_str(&token);
            }
        }

        Ok(full_text)
    }
}

pub fn parse_sse_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("data:") {
        return None;
    }
    let data = trimmed.trim_start_matches("data:").trim();
    if data == "[DONE]" || data.is_empty() {
        return None;
    }
    if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                if !content.is_empty() {
                    return Some(content);
                }
            }
        }
    }
    None
}

pub fn parse_sse_text<F>(sse_text: &str, mut on_chunk: F) -> String
where
    F: FnMut(&str),
{
    let mut full_response = String::new();
    for line in sse_text.lines() {
        if let Some(token) = parse_sse_line(line) {
            on_chunk(&token);
            full_response.push_str(&token);
        }
    }
    full_response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_structure_and_prompt_assembly() {
        let client = OllamaClient::new(None, None);
        assert_eq!(client.endpoint, "http:\x2F\x2Flocalhost:11434");
        assert_eq!(client.model, "bench-reason-4b");
        assert!((client.temperature - 0.2).abs() < f32::EPSILON);

        let req = client.build_request("payload test");
        assert_eq!(req.model, "bench-reason-4b");
        assert!((req.temperature - 0.2).abs() < f32::EPSILON);
        assert!(req.stream);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert!(req.messages[0].content.contains("THE DATA JOURNEY"));
        assert!(req
            .messages[0]
            .content
            .contains("ARCHITECTURAL PATTERN & DESIGN INTENT"));
        assert!(req
            .messages[0]
            .content
            .contains("LANGUAGE & FRAMEWORK CAVEATS"));
        assert!(req
            .messages[0]
            .content
            .contains("CRITICAL ANCHORS & UNHANDLED EDGE CASES"));
        assert_eq!(req.messages[1].role, "user");
        assert_eq!(req.messages[1].content, "payload test");

        let json = serde_json::to_string(&req).expect("request should serialize to json");
        assert!(json.contains("\"temperature\":0.2"));
        assert!(json.contains("\"stream\":true"));
        assert!(json.contains("bench-reason-4b"));
    }

    #[test]
    fn test_sse_chunk_parser() {
        let sse_data = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"World!\"}}]}\n\ndata: [DONE]\n";
        let mut emitted = Vec::new();
        let assembled = parse_sse_text(sse_data, |chunk| {
            emitted.push(chunk.to_string());
        });

        assert_eq!(emitted, vec!["Hello ", "World!"]);
        assert_eq!(assembled, "Hello World!");

        let single_line = "data: {\"choices\":[{\"delta\":{\"content\":\"token\"}}]}";
        assert_eq!(parse_sse_line(single_line), Some("token".to_string()));
        assert_eq!(parse_sse_line("data: [DONE]"), None);
        assert_eq!(parse_sse_line(": ping"), None);
    }

    #[tokio::test]
    async fn test_health_check_diagnostic_formatting() {
        let client = OllamaClient::new(Some("http:\x2F\x2F127.0.0.1:59999".to_string()), None);
        let result = client.check_health().await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Ollama daemon unreachable at http:\x2F\x2F127.0.0.1:59999"));
        assert!(err_msg.contains(
            "Start the service with 'ollama serve' or launch the Ollama application."
        ));
    }
}
