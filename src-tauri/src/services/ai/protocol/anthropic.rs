//! Anthropic-compatible messages protocol implementation.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::ChatProtocol;
use crate::models::AiTestResult;

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Serialize)]
struct MessageRequest {
    model: String,
    messages: Vec<Message>,
    system: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MessageResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

pub struct AnthropicChatProtocol {
    client: reqwest::Client,
    api_key: String,
    api_base_url: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    default_prompt: String,
}

impl AnthropicChatProtocol {
    pub fn new(
        api_key: String,
        api_base_url: String,
        model: String,
        temperature: f32,
        max_tokens: u32,
        default_prompt: String,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            api_base_url,
            model,
            temperature,
            max_tokens,
            default_prompt,
        }
    }

    async fn send_message(
        &self,
        system_prompt: Option<&str>,
        user_message: &str,
    ) -> Result<String> {
        let request = MessageRequest {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".into(),
                content: user_message.into(),
            }],
            system: system_prompt.unwrap_or(&self.default_prompt).to_string(),
            max_tokens: self.max_tokens.max(1),
            temperature: Some(self.temperature),
        };

        let url = format!("{}/messages", self.api_base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .headers(self.build_headers()?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        let completion: MessageResponse = response.json().await?;
        let text = completion
            .content
            .into_iter()
            .filter(|block| block.kind == "text")
            .filter_map(|block| block.text)
            .collect::<Vec<_>>()
            .join("");

        if text.is_empty() {
            return Err(anyhow!("Empty response"));
        }

        Ok(text)
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        if !self.api_key.trim().is_empty() {
            headers.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(&self.api_key)?,
            );
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))?,
            );
        }

        Ok(headers)
    }
}

#[async_trait]
impl ChatProtocol for AnthropicChatProtocol {
    async fn chat_with_system(&self, system_prompt: &str, user_message: &str) -> Result<String> {
        self.send_message(Some(system_prompt), user_message).await
    }

    async fn test_connection(&self, provider: &str) -> Result<AiTestResult> {
        let started = Instant::now();
        self.send_message(Some("Reply with OK only."), "OK").await?;
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
        "anthropic"
    }
}
