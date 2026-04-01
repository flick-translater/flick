//! AI translation gateway with provider and protocol indirection.

mod protocol;
mod provider;

use crate::models::{AISettings, AiTestResult, TranslateRequest, TranslateResponse};
use protocol::create_chat_protocol;
use provider::resolve_provider;
use std::time::Instant;

pub struct TranslationGateway {
    ai_settings: AISettings,
}

impl TranslationGateway {
    pub fn new(ai_settings: AISettings) -> Self {
        Self { ai_settings }
    }

    pub async fn translate(&self, request: TranslateRequest) -> anyhow::Result<TranslateResponse> {
        let provider = resolve_provider(&self.ai_settings.active_provider, &self.ai_settings)?;
        let protocol = create_chat_protocol(provider.protocol, provider.key, provider.settings)?;
        let system_prompt = format!(
            "Translate the following text from {} to {}. Only output the translated text, nothing else.",
            request
                .source_language
                .as_deref()
                .unwrap_or("auto-detected"),
            request.target_language
        );

        let translated_text = protocol
            .chat_with_system(&system_prompt, &request.text)
            .await?;

        Ok(TranslateResponse {
            provider: provider.key.to_string(),
            translated_text,
            detected_source_language: request.source_language,
        })
    }

    pub async fn test_connection(&self) -> anyhow::Result<AiTestResult> {
        let provider = resolve_provider(&self.ai_settings.active_provider, &self.ai_settings)?;
        let protocol = create_chat_protocol(provider.protocol, provider.key, provider.settings)?;
        let started = Instant::now();
        match protocol.test_connection(provider.key).await {
            Ok(result) => Ok(result),
            Err(error) => Ok(AiTestResult {
                ok: false,
                provider: provider.key.to_string(),
                protocol: protocol.protocol_name().to_string(),
                model: provider.settings.model.clone(),
                latency_ms: started.elapsed().as_millis() as u64,
                message: error.to_string(),
            }),
        }
    }
}
