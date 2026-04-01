//! Thin Tauri command adapters for translation.

use tauri::State;

use crate::{
    app::AppState,
    error::FlickError,
    features::translation,
    models::{AISettings, AiTestResult, TranslateRequest, TranslateResponse},
    services::TranslationGateway,
};

#[tauri::command]
pub async fn translate(
    state: State<'_, AppState>,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    translation::run(&state, request).await
}

#[tauri::command]
pub async fn test_ai_connection(ai_settings: AISettings) -> Result<AiTestResult, FlickError> {
    TranslationGateway::new(ai_settings)
        .test_connection()
        .await
        .map_err(Into::into)
}
