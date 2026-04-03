//! Platform bridge for capture-session behavior.
//!
//! This layer covers the parts that differ by OS beyond raw image capture, such as overlay
//! cleanup, main-window suppression, and snapshot preparation.

use tauri::{AppHandle, State};

use crate::app::AppState;

#[cfg(target_os = "linux")]
#[path = "platform/linux_platform.rs"]
mod linux_platform;
#[cfg(target_os = "macos")]
#[path = "platform/macos_platform.rs"]
mod macos_platform;
#[cfg(target_os = "windows")]
#[path = "platform/windows_platform.rs"]
mod windows_platform;

use image::{ImageBuffer, Rgba};

use crate::{
    error::FlickError,
    models::SelectionRect,
    services::{CachedScreenCapture, ScreenCaptureService},
};

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
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::complete_ui_before_capture_processing(app, state);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::complete_ui_before_capture_processing(app, state)
    }

    #[cfg(target_os = "windows")]
    {
        windows_platform::complete_ui_before_capture_processing(app, state)
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
