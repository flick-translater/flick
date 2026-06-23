//! Thin Tauri command adapters for the capture feature.

use tauri::{AppHandle, State};

use crate::{
    app::{AppState, CaptureIntent},
    error::FlickError,
    features::capture,
    models::{CaptureHistory, CaptureRecord, LongCaptureUpdate, StorageInfo},
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

#[tauri::command]
pub fn start_capture_session(app: AppHandle, state: State<'_, AppState>) -> Result<(), FlickError> {
    capture::begin_capture_session(&app, &state)
}

#[tauri::command]
pub fn start_translate_capture_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), FlickError> {
    capture::begin_capture_session_with_intent(&app, &state, CaptureIntent::Translate)
}

#[tauri::command]
pub fn get_pending_capture_image(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, FlickError> {
    capture::get_pending_capture_image(state, session_id)
}

#[tauri::command]
pub fn confirm_regular_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    png_base64: String,
) -> Result<CaptureRecord, FlickError> {
    capture::confirm_regular_capture_edit(app, state, session_id, png_base64)
}

#[tauri::command]
pub fn save_regular_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    png_base64: String,
) -> Result<CaptureRecord, FlickError> {
    capture::save_regular_capture_edit(app, state, session_id, png_base64)
}

#[tauri::command]
pub fn cancel_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    capture::cancel_capture_edit_command(app, state, session_id)
}

#[tauri::command]
pub fn capture_editor_ready(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    capture::capture_editor_ready(app, state, session_id)
}

#[tauri::command]
pub fn start_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<LongCaptureUpdate, FlickError> {
    capture::start_long_capture(app, state, session_id)
}

#[tauri::command]
pub fn get_long_capture_image(session_id: String) -> Result<String, FlickError> {
    capture::get_long_capture_image(session_id)
}

#[tauri::command]
pub fn save_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    capture::save_long_capture(app, state, session_id)
}

#[tauri::command]
pub fn confirm_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    capture::confirm_long_capture(app, state, session_id)
}

#[tauri::command]
pub fn cancel_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    capture::cancel_long_capture(app, state, session_id)
}

#[tauri::command]
pub fn prepare_long_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    capture::prepare_long_capture_edit(app, state, session_id)
}

#[tauri::command]
pub fn open_long_capture_edit_window(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    capture::open_long_capture_edit_window(app, state, session_id)
}

#[tauri::command]
pub fn capture_editor_frontend_log(message: String) {
    capture::capture_editor_frontend_log(&message);
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
