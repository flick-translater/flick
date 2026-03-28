use tauri::State;

use crate::{
    app::AppState,
    error::FlickError,
    features::ocr,
    models::{OcrRequest, OcrResponse},
};

#[tauri::command]
pub fn mock_ocr(
    state: State<'_, AppState>,
    request: OcrRequest,
) -> Result<OcrResponse, FlickError> {
    ocr::run(&state, request)
}
