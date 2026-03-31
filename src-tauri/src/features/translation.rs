//! Translation feature entry points.

use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app::{windows, AppState},
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
    service.translate(request).map_err(Into::into)
}

pub fn emit_translation_ready(
    app: &AppHandle,
    image_path: &str,
    source_text: &str,
    translation: TranslateResponse,
) -> Result<(), FlickError> {
    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
        "translatedText": translation.translated_text,
        "provider": translation.provider,
        "detectedSourceLanguage": translation.detected_source_language,
        "targetLanguage": "zh",
    });

    windows::ensure_widget_window(app)?;
    windows::show_widget_window(app)?;

    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.emit("translation-ready", payload.clone());
    }

    let _ = app.emit("translation-ready", payload);

    Ok(())
}
