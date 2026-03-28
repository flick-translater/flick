//! Platform bridge for capture-session behavior.
//!
//! This layer covers the parts that differ by OS beyond raw image capture, such as overlay
//! cleanup, main-window suppression, and snapshot preparation.

use tauri::{AppHandle, State};

use crate::app::AppState;

#[cfg(target_os = "macos")]
mod macos;
mod non_macos;

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
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            return macos::begin_interactive_capture_session(app, state);
        }
    }

    non_macos::begin_interactive_capture_session(app, state)
}

pub fn cancel_interactive_capture(app: &AppHandle, state: &State<'_, AppState>) {
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            macos::cancel_interactive_capture_session(app, state);
            return;
        }
    }

    non_macos::cancel_interactive_capture_session(app, state);
}

pub fn prepare_for_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            return macos::prepare_for_capture_session(app, state);
        }
    }

    non_macos::prepare_for_capture_session(app, state)
}

pub fn complete_ui_before_capture_processing(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            return macos::complete_ui_before_capture_processing(app, state);
        }
    }

    non_macos::complete_ui_before_capture_processing(app, state)
}

pub fn finalize_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            macos::finalize_capture_session(app, state, restore_previous_frontmost);
            return;
        }
    }

    non_macos::finalize_capture_session(app, state, restore_previous_frontmost);
}

pub fn restore_after_failed_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            macos::restore_after_failed_capture(app, state, restore_previous_frontmost);
            return;
        }
    }

    non_macos::restore_after_failed_capture(app, state, restore_previous_frontmost);
}

pub fn cleanup_after_cancel(app: &AppHandle, state: &State<'_, AppState>) {
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            macos::cleanup_after_cancel(app, state);
            return;
        }
    }

    non_macos::cleanup_after_cancel(app, state);
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
