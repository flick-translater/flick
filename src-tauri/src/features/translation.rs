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

pub fn show_window_immediately(app: &AppHandle, image_path: &str) -> Result<(), FlickError> {
    windows::ensure_widget_window(app)?;
    windows::show_widget_window(app)?;

    let payload = serde_json::json!({
        "imagePath": image_path,
        "loading": true,
    });

    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.emit("ocr-loading", payload.clone());
    }

    Ok(())
}

pub fn emit_ocr_ready(
    app: &AppHandle,
    image_path: &str,
    source_text: &str,
) -> Result<(), FlickError> {
    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
    });

    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.emit("ocr-ready", payload.clone());
    }

    let _ = app.emit("ocr-ready", payload);

    Ok(())
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

    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.emit("translation-ready", payload.clone());
    }

    let _ = app.emit("translation-ready", payload);

    Ok(())
}
