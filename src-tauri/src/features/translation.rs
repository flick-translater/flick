//! Translation feature entry points.

use tauri::{AppHandle, Emitter};

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{TranslateRequest, TranslateResponse},
    services::TranslationService,
};

pub fn run(state: &AppState, request: TranslateRequest) -> Result<TranslateResponse, FlickError> {
    run_with_service(state.translation_service.as_ref(), request)
}

pub fn run_with_service(
    service: &dyn TranslationService,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    // Keeping translation behind a trait lets the widget flow switch providers without API churn.
    service.translate(request).map_err(Into::into)
}

pub fn emit_translation_ready(
    app: &AppHandle,
    image_path: &str,
    source_text: &str,
    translation: TranslateResponse,
) -> Result<(), FlickError> {
    // Widget payload shape stays centralized here so command/session code does not duplicate it.
    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
        "translatedText": translation.translated_text,
        "provider": translation.provider,
        "detectedSourceLanguage": translation.detected_source_language,
        "targetLanguage": "zh",
    });
    let widget = windows::ensure_widget_window(app)?;
    windows::show_widget_window(app)?;
    let _ = widget.emit("translation-ready", payload);
    Ok(())
}
