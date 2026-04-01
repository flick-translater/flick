//! Protocol adapters used by provider backends.

mod anthropic;
mod openai;

use anyhow::Result;
use async_trait::async_trait;

use crate::models::{AiTestResult, ProviderSettings};

pub use anthropic::AnthropicChatProtocol;
pub use openai::OpenAiChatProtocol;

#[derive(Clone, Copy)]
pub enum ProtocolKind {
    OpenAi,
    Anthropic,
}

#[async_trait]
pub trait ChatProtocol: Send + Sync {
    async fn chat_with_system(&self, system_prompt: &str, user_message: &str) -> Result<String>;
    async fn test_connection(&self, provider: &str) -> Result<AiTestResult>;
    fn protocol_name(&self) -> &'static str;
}

pub fn create_chat_protocol(
    protocol: ProtocolKind,
    _provider_key: &str,
    settings: &ProviderSettings,
) -> Result<Box<dyn ChatProtocol>> {
    match protocol {
        ProtocolKind::OpenAi => Ok(Box::new(OpenAiChatProtocol::new(
            _provider_key.to_string(),
            settings.api_key.clone(),
            settings.api_base_url.clone(),
            settings.model.clone(),
            settings.temperature,
            settings.max_tokens,
            settings.default_prompt.clone(),
        ))),
        ProtocolKind::Anthropic => Ok(Box::new(AnthropicChatProtocol::new(
            settings.api_key.clone(),
            settings.api_base_url.clone(),
            settings.model.clone(),
            settings.temperature,
            settings.max_tokens,
            settings.default_prompt.clone(),
        ))),
    }
}
