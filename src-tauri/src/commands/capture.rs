//! Thin Tauri command adapters for the capture feature.

use tauri::{AppHandle, State};

use crate::{
    app::{AppState, CaptureIntent},
    error::FlickError,
    features::capture,
    models::{CaptureHistory, StorageInfo},
};

#[tauri::command]
pub fn list_capture_history(state: State<'_, AppState>) -> Result<CaptureHistory, FlickError> {
    capture::list_capture_history(&state)
}

#[tauri::command]
pub fn get_storage_info(state: State<'_, AppState>) -> Result<StorageInfo, FlickError> {
    capture::get_storage_info(&state)
}

#[tauri::command]
pub fn pick_screenshot_directory() -> Result<Option<String>, FlickError> {
    capture::pick_screenshot_directory()
}

#[tauri::command]
pub fn open_file_in_default_app(path: String) -> Result<(), FlickError> {
    capture::open_file_in_default_app(&path)
}

#[tauri::command]
pub fn read_image_as_data_url(path: String) -> Result<String, FlickError> {
    capture::read_image_as_data_url(&path)
}

#[tauri::command]
pub fn delete_capture(state: State<'_, AppState>, path: String) -> Result<(), FlickError> {
    capture::delete_capture(&state, &path)
}

#[tauri::command]
pub fn clear_all_captures(state: State<'_, AppState>) -> Result<(), FlickError> {
    capture::clear_all_captures(&state)
}

#[tauri::command]
pub fn copy_capture_image(path: String) -> Result<(), FlickError> {
    capture::copy_capture_image(&path)
}

pub fn begin_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    capture::begin_capture_session(app, state)
}

pub fn begin_capture_session_with_intent(
    app: &AppHandle,
    state: &State<'_, AppState>,
    intent: CaptureIntent,
) -> Result<(), FlickError> {
    capture::begin_capture_session_with_intent(app, state, intent)
}
