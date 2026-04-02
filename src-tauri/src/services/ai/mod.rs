//! AI translation gateway with provider and protocol indirection.

mod protocol;
mod provider;

use crate::models::{
    AISettings, AiTestResult, DEFAULT_TRANSLATION_PROMPT, TranslateRequest, TranslateResponse,
};
use protocol::create_chat_protocol;
use provider::resolve_provider;
use std::time::Instant;

pub struct TranslationGateway {
    ai_settings: AISettings,
}

const TRANSLATION_SYSTEM_PROMPT: &str = "You are a professional translator.";

impl TranslationGateway {
    pub fn new(ai_settings: AISettings) -> Self {
        Self { ai_settings }
    }

    pub async fn translate(&self, request: TranslateRequest) -> anyhow::Result<TranslateResponse> {
        let provider = resolve_provider(&self.ai_settings.active_provider, &self.ai_settings)?;
        let protocol = create_chat_protocol(provider.protocol, provider.key, provider.settings)?;
        let user_prompt_template = provider.settings.default_prompt.trim();
        let user_prompt = render_translation_prompt(
            if user_prompt_template.is_empty() {
                DEFAULT_TRANSLATION_PROMPT
            } else {
                user_prompt_template
            },
            &request,
        );

        let translated_text = protocol
            .chat_with_system(TRANSLATION_SYSTEM_PROMPT, &user_prompt)
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

fn render_translation_prompt(template: &str, request: &TranslateRequest) -> String {
    let source_language = request
        .source_language
        .as_deref()
        .unwrap_or("auto-detected");

    template
        .replace("${source}", request.text.as_str())
        .replace("${source.lang}", source_language)
        .replace("${target.lang}", request.target_language.as_str())
}
