//! Non-macOS capture-session behavior.

use tauri::{AppHandle, State};

use crate::{
    app::AppState,
    error::FlickError,
    services::CachedScreenCapture,
};

pub fn begin_interactive_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    Err(FlickError::Message(
        "capture interaction is not implemented on this platform".into(),
    ))
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, _state: &State<'_, AppState>) {}

pub fn prepare_for_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    Err(FlickError::Message(
        "capture is not implemented on this platform".into(),
    ))
}

pub fn complete_ui_before_capture_processing(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    Err(FlickError::Message(
        "capture is not implemented on this platform".into(),
    ))
}

pub fn finalize_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
}

pub fn restore_after_failed_capture(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
}

pub fn cleanup_after_cancel(_app: &AppHandle, _state: &State<'_, AppState>) {}
