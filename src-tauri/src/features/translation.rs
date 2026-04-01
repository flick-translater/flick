use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{AISettings, TranslateRequest, TranslateResponse},
    services::TranslationGateway,
};

pub async fn run(
    state: &AppState,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    let ai_settings = state
        .settings
        .lock()
        .map_err(|_| FlickError::LockError("settings".into()))?
        .ai
        .clone();
    run_with_ai_settings(&ai_settings, request).await
}

pub async fn run_with_ai_settings(
    ai_settings: &AISettings,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    TranslationGateway::new(ai_settings.clone())
        .translate(request)
        .await
        .map_err(Into::into)
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
    ocr_detected_source_language: Option<&str>,
    auto_translate_enabled: bool,
    target_language: &str,
) -> Result<(), FlickError> {
    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
        "ocrDetectedSourceLanguage": ocr_detected_source_language,
        "autoTranslateEnabled": auto_translate_enabled,
        "targetLanguage": target_language,
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
    target_language: &str,
    translation: TranslateResponse,
) -> Result<(), FlickError> {
    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
        "translatedText": translation.translated_text,
        "provider": translation.provider,
        "detectedSourceLanguage": translation.detected_source_language,
        "targetLanguage": target_language,
    });

    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.emit("translation-ready", payload.clone());
    }

    let _ = app.emit("translation-ready", payload);

    Ok(())
}
