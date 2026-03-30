//! macOS capture backend based on Core Graphics.

use anyhow::anyhow;
use image::{ImageBuffer, Rgba};

use crate::{models::SelectionRect, services::screen_capture::MacosCaptureBackend};

pub struct CoreGraphicsCaptureBackend;

impl MacosCaptureBackend for CoreGraphicsCaptureBackend {
    fn name(&self) -> &'static str {
        "CoreGraphics"
    }

    fn capture_selection(
        &self,
        selection: &SelectionRect,
    ) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        capture_selection(selection)
    }

    fn capture_desktop_snapshot(
        &self,
        bounds: &SelectionRect,
    ) -> anyhow::Result<crate::services::CachedScreenCapture> {
        super::macos_frozen::capture_desktop_snapshot_with_core_graphics(bounds)
    }
}

pub fn capture_selection(
    selection: &SelectionRect,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    use core_graphics::{
        geometry::{CGPoint, CGRect, CGSize},
        window::{
            create_image, kCGNullWindowID, kCGWindowImageDefault, kCGWindowListOptionOnScreenOnly,
        },
    };

    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    let rect = CGRect::new(
        &CGPoint::new(selection.x as f64, selection.y as f64),
        &CGSize::new(selection.width as f64, selection.height as f64),
    );

    let cg_image = create_image(
        rect,
        kCGWindowListOptionOnScreenOnly,
        kCGNullWindowID,
        kCGWindowImageDefault,
    )
    .ok_or_else(|| anyhow!("failed to capture on-screen selection"))?;

    let width = cg_image.width();
    let height = cg_image.height();
    // Core Graphics may pad each row, so normalize the buffer before converting color channels.
    let clean_buf = remove_extra_data(
        width,
        height,
        cg_image.bytes_per_row(),
        cg_image.data().bytes().to_vec(),
    );

    bgra_to_rgba_image(width as u32, height as u32, clean_buf)
}

fn bgra_to_rgba_image(
    width: u32,
    height: u32,
    buf: Vec<u8>,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let mut rgba_buf = buf.clone();

    for (src, dst) in buf.chunks_exact(4).zip(rgba_buf.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    ImageBuffer::from_vec(width, height, rgba_buf).ok_or_else(|| anyhow!("buffer not big enough"))
}

fn remove_extra_data(width: usize, height: usize, bytes_per_row: usize, buf: Vec<u8>) -> Vec<u8> {
    let extra_bytes_per_row = bytes_per_row - width * 4;
    let mut result = Vec::with_capacity(buf.len().saturating_sub(extra_bytes_per_row * height));
    for row in buf.chunks_exact(bytes_per_row) {
        result.extend_from_slice(&row[..width * 4]);
    }
    result
}
