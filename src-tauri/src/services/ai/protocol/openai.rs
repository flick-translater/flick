//! OpenAI-compatible chat protocol implementation.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{
    Response,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use serde::{Deserialize, Serialize};
use std::error::Error as _;
use std::time::Instant;

use super::ChatProtocol;
use crate::models::AiTestResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChatChunkChoice>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatChunkChoice {
    delta: ChatChunkDelta,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ChatChunkDelta {
    content: Option<String>,
}

pub struct OpenAiChatProtocol {
    client: reqwest::Client,
    provider_key: String,
    api_key: String,
    api_base_url: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    default_prompt: String,
}

impl OpenAiChatProtocol {
    pub fn new(
        provider_key: String,
        api_key: String,
        api_base_url: String,
        model: String,
        temperature: f32,
        max_tokens: u32,
        default_prompt: String,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Flick/0.1")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            provider_key,
            api_key,
            api_base_url,
            model,
            temperature,
            max_tokens,
            default_prompt,
        }
    }

    async fn send_chat_request(
        &self,
        system_prompt: Option<&str>,
        user_message: &str,
    ) -> Result<String> {
        let system = system_prompt.unwrap_or(&self.default_prompt).to_string();
        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: system,
                },
                ChatMessage {
                    role: "user".into(),
                    content: user_message.into(),
                },
            ],
            stream: true,
            temperature: Some(self.temperature),
            max_tokens: Some(self.max_tokens),
        };

        let url = format!(
            "{}/chat/completions",
            self.api_base_url.trim_end_matches('/')
        );
        let mut builder = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json");

        if !self.api_key.trim().is_empty() {
            builder = builder.header(AUTHORIZATION, format!("Bearer {}", self.api_key));
        }

        println!(
            "[translation-http] provider={} url={} stream={} api_key_present={}",
            self.provider_key,
            url,
            request.stream,
            !self.api_key.trim().is_empty()
        );

        let response = match builder.json(&request).send().await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("[translation-http] request error: {error}");
                eprintln!("[translation-http] request error debug: {error:?}");
                let mut index = 0;
                let mut source = error.source();
                while let Some(cause) = source {
                    eprintln!("[translation-http] cause[{index}]: {cause}");
                    source = cause.source();
                    index += 1;
                }
                return Err(error.into());
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));

        if is_sse {
            return self.collect_streaming_response(response).await;
        }

        let completion: ChatCompletionResponse = response.json().await?;
        completion
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .ok_or_else(|| anyhow!("Empty response"))
    }

    async fn collect_streaming_response(&self, response: Response) -> Result<String> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregated = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(std::str::from_utf8(&chunk)?);

            while let Some(index) = buffer.find('\n') {
                let line = buffer[..index].trim_end_matches('\r').to_string();
                buffer.drain(..=index);
                if self.consume_sse_line(&line, &mut aggregated)? {
                    return if aggregated.is_empty() {
                        Err(anyhow!("Empty response"))
                    } else {
                        Ok(aggregated)
                    };
                }
            }
        }

        let trailing = buffer.trim();
        if !trailing.is_empty() {
            let _ = self.consume_sse_line(trailing, &mut aggregated)?;
        }

        if aggregated.is_empty() {
            return Err(anyhow!("Empty response"));
        }

        Ok(aggregated)
    }

    fn consume_sse_line(&self, line: &str, aggregated: &mut String) -> Result<bool> {
        let line = line.trim();
        if line.is_empty() || !line.starts_with("data:") {
            return Ok(false);
        }

        let payload = line.trim_start_matches("data:").trim();
        if payload == "[DONE]" {
            return Ok(true);
        }

        let chunk: ChatCompletionChunk = serde_json::from_str(payload)?;
        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                aggregated.push_str(&content);
            }
        }

        Ok(false)
    }
}

#[async_trait]
impl ChatProtocol for OpenAiChatProtocol {
    async fn chat_with_system(&self, system_prompt: &str, user_message: &str) -> Result<String> {
        self.send_chat_request(Some(system_prompt), user_message)
            .await
    }

    async fn test_connection(&self, provider: &str) -> Result<AiTestResult> {
        let started = Instant::now();
        self.send_chat_request(Some("Reply with OK only."), "OK")
            .await?;
        Ok(AiTestResult {
            ok: true,
            provider: provider.to_string(),
            protocol: self.protocol_name().to_string(),
            model: self.model.clone(),
            latency_ms: started.elapsed().as_millis() as u64,
            message: "Connection successful".into(),
        })
    }

    fn protocol_name(&self) -> &'static str {
        "openai"
    }
}
