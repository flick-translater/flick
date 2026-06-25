use tauri::{AppHandle, Manager};

use crate::error::FlickError;

pub(super) fn set_recording_window_mode(
    app: &AppHandle,
    session_id: &str,
    recording: bool,
) -> Result<(), FlickError> {
    if let Some(window) = screenshot_editor_window(app, session_id) {
        let _ = window.set_ignore_cursor_events(recording);
    }
    Ok(())
}

pub(super) fn prepare_recording_capture_visibility(_app: &AppHandle, _session_id: &str) {}

pub(super) fn cleanup_recording_capture_visibility(_app: &AppHandle, _session_id: &str) {}

fn screenshot_editor_window(app: &AppHandle, session_id: &str) -> Option<tauri::WebviewWindow> {
    let session_label = format!("screenshot-editor-{session_id}");
    app.get_webview_window(&session_label)
        .or_else(|| app.get_webview_window("screenshot-editor-preload"))
}
