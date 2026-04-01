use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{AISettings, TranslateRequest, TranslateResponse, TranslationHistory},
    services::{NewTranslationRecord, TranslationGateway},
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

pub fn save_history(
    state: &AppState,
    request: &TranslateRequest,
    response: &TranslateResponse,
    image_path: Option<&str>,
) -> Result<(), FlickError> {
    state
        .translation_history_store
        .insert_record(NewTranslationRecord {
            source_text: &request.text,
            translated_text: &response.translated_text,
            source_language: response
                .detected_source_language
                .as_deref()
                .or(request.source_language.as_deref()),
            target_language: &request.target_language,
            provider: &response.provider,
            image_path,
        })
        .map_err(Into::into)
}

pub fn list_history(state: &AppState) -> Result<TranslationHistory, FlickError> {
    Ok(TranslationHistory {
        database_path: state
            .translation_history_store
            .db_path()
            .display()
            .to_string(),
        items: state.translation_history_store.list_records()?,
    })
}

pub fn clear_history(state: &AppState) -> Result<(), FlickError> {
    state.translation_history_store.clear().map_err(Into::into)
}

pub fn delete_history_record(state: &AppState, id: i64) -> Result<(), FlickError> {
    state
        .translation_history_store
        .delete_record(id)
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
