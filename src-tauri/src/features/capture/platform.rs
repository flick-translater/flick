#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod non_macos;

use image::{ImageBuffer, Rgba};

use crate::{
    error::FlickError,
    models::SelectionRect,
    services::{CachedScreenCapture, ScreenCaptureService},
};

#[cfg(target_os = "macos")]
pub use macos::{
    cleanup_after_cancel, complete_ui_before_capture_processing, current_global_cursor_position,
    finalize_capture_session, prepare_for_capture_session, restore_after_failed_capture,
};
#[cfg(not(target_os = "macos"))]
pub use non_macos::{
    cleanup_after_cancel, complete_ui_before_capture_processing, current_global_cursor_position,
    finalize_capture_session, prepare_for_capture_session, restore_after_failed_capture,
};

pub fn capture_image(
    capture_service: &ScreenCaptureService,
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, FlickError> {
    capture_service
        .capture_selection(selection, cached_screens)
        .map_err(Into::into)
}
