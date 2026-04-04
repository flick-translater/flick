//! Linux screenshot service facade.

#[path = "linux_wayland_platform.rs"]
mod linux_wayland_platform;
#[path = "linux_x11_platform.rs"]
mod linux_x11_platform;

use image::{ImageBuffer, Rgba};

use crate::models::SelectionRect;

use super::CachedScreenCapture;

pub fn capture_selection(
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    if !cached_screens.is_empty() {
        return linux_wayland_platform::capture_selection(selection, cached_screens);
    }

    linux_x11_platform::capture_selection(selection, cached_screens)
}
