//! Thin Tauri command adapters for translation.

use tauri::State;

use crate::{
    app::AppState,
    error::FlickError,
    features::translation,
    models::{TranslateRequest, TranslateResponse},
};

#[tauri::command]
pub fn mock_translate(
    state: State<'_, AppState>,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    translation::run(&state, request)
}
