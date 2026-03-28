//! Thin Tauri command adapters for the capture feature.

use tauri::{AppHandle, State};

use crate::{
    app::{AppState, CaptureIntent},
    error::FlickError,
    features::capture,
    models::{CaptureContext, CaptureHistory, CursorPosition, SelectionRect, StorageInfo},
};

#[tauri::command]
pub fn start_capture(app: AppHandle, state: State<'_, AppState>) -> Result<(), FlickError> {
    capture::begin_capture_session(&app, &state)
}

#[tauri::command]
pub fn focus_capture_window(app: AppHandle, label: String) -> Result<(), FlickError> {
    capture::focus_capture_window(&app, &label)
}

#[tauri::command]
pub fn get_global_cursor_position(app: AppHandle) -> Result<CursorPosition, FlickError> {
    capture::get_global_cursor_position(&app)
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) -> Result<(), FlickError> {
    capture::cancel_capture(&app)
}

#[tauri::command]
pub fn complete_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    selection: SelectionRect,
) -> Result<(), FlickError> {
    capture::complete_capture(&app, &state, selection)
}

#[tauri::command]
pub fn refresh_capture_context(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
) -> Result<CaptureContext, FlickError> {
    capture::refresh_capture_context(&app, &state, &label)
}

#[tauri::command]
pub fn get_capture_context(
    state: State<'_, AppState>,
    label: String,
) -> Result<CaptureContext, FlickError> {
    capture::get_capture_context(&state, &label)
}

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
