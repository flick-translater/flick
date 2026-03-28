use tauri::{AppHandle, Manager, State};

use crate::{
    app::AppState, error::FlickError, models::CursorPosition, services::CachedScreenCapture,
};

pub fn current_global_cursor_position(_app: &AppHandle) -> Result<CursorPosition, FlickError> {
    Err(FlickError::Message(
        "global cursor position is only implemented on macOS".into(),
    ))
}

pub fn prepare_for_capture_session(
    _app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let snapshots = state.capture_service.capture_all_screens()?;
    let mut guard = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshots mutex poisoned".into()))?;
    *guard = snapshots;
    Ok(())
}

pub fn complete_ui_before_capture_processing(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    super::super::session::emit_capture_event_to_windows(app, "capture-ended", "finished");
    for (_, window) in app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| crate::app::windows::is_capture_window_label(label))
    {
        window.hide()?;
    }

    let mut guard = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshots mutex poisoned".into()))?;
    let snapshots = guard.clone();
    guard.clear();
    Ok(snapshots)
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
