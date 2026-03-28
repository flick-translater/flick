//! Translation service abstraction plus the current mock implementation.

use crate::models::{TranslateRequest, TranslateResponse};

pub trait TranslationService: Send + Sync {
    fn translate(&self, request: TranslateRequest) -> anyhow::Result<TranslateResponse>;
}

pub struct MockTranslationService;

impl TranslationService for MockTranslationService {
    fn translate(&self, request: TranslateRequest) -> anyhow::Result<TranslateResponse> {
        // The mock echoes enough structure for the widget and command flow to be exercised.
        Ok(TranslateResponse {
            provider: "mock-translate".into(),
            translated_text: format!("[{}] {}", request.target_language, request.text.trim()),
            detected_source_language: request.source_language.or(Some("auto".into())),
        })
    }
}
