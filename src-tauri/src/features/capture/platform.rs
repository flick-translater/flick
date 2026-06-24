//! Platform bridge for capture-session behavior.
//!
//! This layer covers the parts that differ by OS beyond raw image capture, such as overlay
//! cleanup, main-window suppression, and snapshot preparation.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64},
};

use tauri::{AppHandle, State};

use crate::app::AppState;

#[cfg(target_os = "linux")]
#[path = "platform/linux_long_capture_scroll_platform.rs"]
mod linux_long_capture_scroll_platform;
#[cfg(target_os = "linux")]
#[path = "platform/linux_platform.rs"]
mod linux_platform;
#[cfg(target_os = "macos")]
#[path = "platform/macos_long_capture_scroll_platform.rs"]
mod macos_long_capture_scroll_platform;
#[cfg(target_os = "macos")]
#[path = "platform/macos_platform.rs"]
mod macos_platform;
#[cfg(target_os = "windows")]
#[path = "platform/windows_long_capture_scroll_platform.rs"]
mod windows_long_capture_scroll_platform;
#[cfg(target_os = "windows")]
#[path = "platform/windows_platform.rs"]
mod windows_platform;

use image::{ImageBuffer, Rgba};

use crate::{
    error::FlickError,
    models::SelectionRect,
    services::{CachedScreenCapture, ScreenCaptureService},
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScrollTarget {
    /// Foreground application process id at the moment the capture session started.
    ///
    /// macOS currently uses this as an availability signal before posting at HID scope. Windows can
    /// extend this type with an HWND/thread id; X11 can add a window id/display handle. Wayland
    /// should generally report unsupported.
    pub pid: Option<i32>,
}

pub(crate) struct ScrollControllerOptions {
    pub app: AppHandle,
    pub session_id: String,
    pub selection: SelectionRect,
    pub stop: Arc<AtomicBool>,
    pub cursor_passthrough: Arc<AtomicBool>,
    pub last_scroll_millis: Arc<AtomicI64>,
    pub last_scroll_delta: Arc<AtomicI64>,
    pub target: ScrollTarget,
    pub should_throttle_scroll: Arc<dyn Fn() -> bool + Send + Sync>,
}

pub fn start_interactive_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::begin_interactive_capture_session(app, state);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::begin_interactive_capture_session(app, state)
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::begin_interactive_capture_session(app, state)
    }
}

pub(crate) fn start_scroll_controller(options: ScrollControllerOptions) {
    #[cfg(target_os = "macos")]
    {
        macos_long_capture_scroll_platform::start_scroll_controller(options);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        linux_long_capture_scroll_platform::start_scroll_controller(options);
    }

    #[cfg(target_os = "windows")]
    {
        windows_long_capture_scroll_platform::start_scroll_controller(options);
    }
}

pub(crate) fn start_long_capture_button_scroll(
    app: AppHandle,
    session_id: String,
    selection: SelectionRect,
    target: ScrollTarget,
    direction: i32,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) -> Result<(), FlickError> {
    #[cfg(target_os = "macos")]
    {
        return macos_long_capture_scroll_platform::start_button_scroll(
            app, session_id, selection, target, direction, stop, running,
        );
    }

    #[cfg(target_os = "linux")]
    {
        return linux_long_capture_scroll_platform::start_button_scroll(
            app, session_id, selection, target, direction, stop, running,
        );
    }

    #[cfg(target_os = "windows")]
    {
        windows_long_capture_scroll_platform::start_button_scroll(
            app, session_id, selection, target, direction, stop, running,
        )
    }
}

pub fn cancel_interactive_capture(app: &AppHandle, state: &State<'_, AppState>) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::cancel_interactive_capture_session(app, state);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::cancel_interactive_capture_session(app, state);
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::cancel_interactive_capture_session(app, state);
    }
}

pub fn prepare_for_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::prepare_for_capture_session(app, state);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::prepare_for_capture_session(app, state)
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::prepare_for_capture_session(app, state)
    }
}

pub fn complete_ui_before_capture_processing(
    app: &AppHandle,
    state: &State<'_, AppState>,
    hide_overlay: bool,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::complete_ui_before_capture_processing(app, state, hide_overlay);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::complete_ui_before_capture_processing(app, state, hide_overlay)
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::complete_ui_before_capture_processing(app, state, hide_overlay)
    }
}

pub fn finalize_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::finalize_capture_session(app, state, restore_previous_frontmost);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::finalize_capture_session(app, state, restore_previous_frontmost);
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::finalize_capture_session(app, state, restore_previous_frontmost);
    }
}

pub fn restore_after_failed_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::restore_after_failed_capture(app, state, restore_previous_frontmost);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::restore_after_failed_capture(app, state, restore_previous_frontmost);
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::restore_after_failed_capture(app, state, restore_previous_frontmost);
    }
}

pub fn cleanup_after_cancel(app: &AppHandle, state: &State<'_, AppState>) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::cleanup_after_cancel(app, state);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::cleanup_after_cancel(app, state);
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::cleanup_after_cancel(app, state);
    }
}

pub fn hide_overlay_for_live_capture(app: &AppHandle, state: &State<'_, AppState>) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::hide_overlay_for_live_capture(app, state);
        return;
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let _ = (app, state);
    }
}

pub fn restore_overlay_after_live_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    selection: &SelectionRect,
) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::restore_overlay_after_live_capture(app, state, selection);
        return;
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let _ = (app, state, selection);
    }
}

pub fn set_overlay_capture_sharing(app: &AppHandle, include_in_capture: bool) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::set_overlay_capture_sharing(app, include_in_capture);
        return;
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let _ = (app, include_in_capture);
    }
}

pub fn set_overlay_mouse_passthrough(app: &AppHandle, passthrough: bool) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::set_overlay_mouse_passthrough(app, passthrough);
        return;
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let _ = (app, passthrough);
    }
}

pub fn set_window_capture_sharing(window: &tauri::WebviewWindow, include_in_capture: bool) {
    #[cfg(target_os = "macos")]
    {
        macos_platform::set_window_capture_sharing(window, include_in_capture);
        return;
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let _ = (window, include_in_capture);
    }
}

pub fn capture_image(
    capture_service: &ScreenCaptureService,
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, FlickError> {
    // Image acquisition still goes through the service facade so the feature layer stays narrow.
    let image = capture_service
        .capture_selection(selection, cached_screens)
        .map_err(FlickError::from)?;
    Ok(image)
}

pub fn editor_escape_key_pressed() -> bool {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::editor_escape_key_pressed();
    }

    #[cfg(target_os = "windows")]
    {
        return windows_platform::editor_escape_key_pressed();
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::editor_escape_key_pressed()
    }
}

pub fn supports_screenshot_editor_toolbar() -> bool {
    #[cfg(target_os = "linux")]
    {
        return linux_platform::supports_screenshot_editor_toolbar();
    }

    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

pub fn keep_native_overlay_until_editor_finish() -> bool {
    #[cfg(target_os = "linux")]
    {
        return linux_platform::supports_screenshot_editor_toolbar();
    }

    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}
