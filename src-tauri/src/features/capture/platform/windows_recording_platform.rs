use tauri::{AppHandle, Manager};

use crate::{error::FlickError, models::SelectionRect};

use super::windows_platform;

pub(super) fn set_recording_window_mode(
    app: &AppHandle,
    session_id: &str,
    recording: bool,
) -> Result<(), FlickError> {
    let Some(window) = screenshot_editor_window(app, session_id) else {
        return Ok(());
    };
    let url = window
        .url()
        .map_err(|error| FlickError::Message(format!("failed to read editor url: {error}")))?;
    let regions = if recording {
        recording_editor_regions(&url)
    } else {
        regular_editor_regions(&url)
    };
    crate::app::platform::configure_screenshot_editor_window_shape(&window, &regions);
    Ok(())
}

pub(super) fn prepare_recording_capture_visibility(app: &AppHandle, session_id: &str) {
    set_recording_capture_visibility(app, session_id, false);
}

pub(super) fn cleanup_recording_capture_visibility(app: &AppHandle, session_id: &str) {
    set_recording_capture_visibility(app, session_id, true);
}

fn set_recording_capture_visibility(app: &AppHandle, session_id: &str, include_in_capture: bool) {
    windows_platform::set_overlay_capture_sharing(app, include_in_capture);
    if let Some(window) = screenshot_editor_window(app, session_id) {
        windows_platform::set_window_capture_sharing(&window, include_in_capture);
    }
    if let Some(window) = recording_controls_window(app, session_id) {
        windows_platform::set_window_capture_sharing(&window, include_in_capture);
    }
}

fn screenshot_editor_window(app: &AppHandle, session_id: &str) -> Option<tauri::WebviewWindow> {
    let session_label = format!("screenshot-editor-{session_id}");
    app.get_webview_window(&session_label)
        .or_else(|| app.get_webview_window("screenshot-editor-preload"))
}

fn recording_controls_window(app: &AppHandle, session_id: &str) -> Option<tauri::WebviewWindow> {
    let label = format!("gif-recording-toolbar-{session_id}");
    app.get_webview_window(&label)
}

fn recording_editor_regions(url: &tauri::Url) -> Vec<SelectionRect> {
    let selection_left = query_f64(url, "selection_left").unwrap_or(0.0);
    let selection_top = query_f64(url, "selection_top").unwrap_or(0.0);
    let width = query_f64(url, "display_width").unwrap_or(1.0).max(1.0);
    let height = query_f64(url, "display_height").unwrap_or(1.0).max(1.0);
    let toolbar_left = query_f64(url, "toolbar_left").unwrap_or(8.0);
    let toolbar_top = query_f64(url, "toolbar_top").unwrap_or(8.0);
    let border = 4.0_f64.min(width).min(height).max(1.0);

    vec![
        rect(selection_left, selection_top, width, border),
        rect(
            selection_left,
            selection_top + height - border,
            width,
            border,
        ),
        rect(selection_left, selection_top, border, height),
        rect(
            selection_left + width - border,
            selection_top,
            border,
            height,
        ),
        rect(toolbar_left, toolbar_top, 240.0, 56.0),
    ]
}

fn regular_editor_regions(url: &tauri::Url) -> Vec<SelectionRect> {
    let selection_left = query_f64(url, "selection_left").unwrap_or(0.0);
    let selection_top = query_f64(url, "selection_top").unwrap_or(0.0);
    let width = query_f64(url, "display_width").unwrap_or(1.0).max(1.0);
    let height = query_f64(url, "display_height").unwrap_or(1.0).max(1.0);
    let toolbar_left = query_f64(url, "toolbar_left").unwrap_or(8.0);
    let toolbar_top = query_f64(url, "toolbar_top").unwrap_or(8.0);
    let thumbnail_left = query_f64(url, "thumbnail_left").unwrap_or(8.0);
    let thumbnail_region_top = query_f64(url, "thumbnail_region_top").unwrap_or(8.0);
    let thumbnail_width = query_f64(url, "thumbnail_width").unwrap_or(300.0);
    let thumbnail_height = query_f64(url, "thumbnail_height").unwrap_or(560.0);

    vec![
        rect(selection_left, selection_top, width, height),
        rect(toolbar_left, toolbar_top - 300.0, 680.0, 340.0),
        rect(
            thumbnail_left,
            thumbnail_region_top,
            thumbnail_width,
            thumbnail_height,
        ),
    ]
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> SelectionRect {
    SelectionRect {
        x: x.floor() as i32,
        y: y.floor().max(0.0) as i32,
        width: width.ceil().max(1.0) as u32,
        height: height.ceil().max(1.0) as u32,
    }
}

fn query_f64(url: &tauri::Url, key: &str) -> Option<f64> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.parse::<f64>().ok())
}
