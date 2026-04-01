//! Thin Tauri command adapters for translation.

use tauri::{AppHandle, Emitter, State};

use crate::{
    app::AppState,
    error::FlickError,
    features::translation,
    models::{AISettings, AiTestResult, TranslateRequest, TranslateResponse, TranslationHistory},
    services::TranslationGateway,
};

#[tauri::command]
pub async fn translate(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    let response = translation::run(&state, request.clone()).await?;
    translation::save_history(&state, &request, &response, None)?;
    let _ = app.emit("translation-history-updated", ());
    Ok(response)
}

#[tauri::command]
pub fn list_translation_history(
    state: State<'_, AppState>,
) -> Result<TranslationHistory, FlickError> {
    translation::list_history(&state)
}

#[tauri::command]
pub fn clear_translation_history(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), FlickError> {
    translation::clear_history(&state)?;
    let _ = app.emit("translation-history-updated", ());
    Ok(())
}

#[tauri::command]
pub fn delete_translation_record(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), FlickError> {
    translation::delete_history_record(&state, id)?;
    let _ = app.emit("translation-history-updated", ());
    Ok(())
}

#[tauri::command]
pub async fn test_ai_connection(ai_settings: AISettings) -> Result<AiTestResult, FlickError> {
    TranslationGateway::new(ai_settings)
        .test_connection()
        .await
        .map_err(Into::into)
}
