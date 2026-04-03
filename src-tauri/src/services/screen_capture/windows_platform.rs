use image::{ImageBuffer, Rgba};

use crate::models::SelectionRect;

use super::CachedScreenCapture;

pub fn capture_selection(
    _selection: &SelectionRect,
    _cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    anyhow::bail!("screen capture is not implemented on Windows")
}
