//! macOS capture backend based on ScreenCaptureKit.

use std::mem;

use anyhow::anyhow;
use core_graphics::image::CGImage;
use foreign_types::ForeignType;
use image::{ImageBuffer, Rgba};
use screencapturekit::{
    cg::CGRect,
    screenshot_manager::{CGImage as ScCgImage, SCScreenshotManager},
};

use crate::{
    models::SelectionRect,
    services::{CachedScreenCapture, screen_capture::MacosCaptureBackend},
};

pub struct ScreenCaptureKitBackend;

impl MacosCaptureBackend for ScreenCaptureKitBackend {
    fn name(&self) -> &'static str {
        "ScreenCaptureKit"
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
    ) -> anyhow::Result<CachedScreenCapture> {
        capture_desktop_snapshot(bounds)
    }
}

pub fn capture_selection(
    selection: &SelectionRect,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    let image = capture_image_in_rect(selection)?;
    rgba_image_from_sckit(image)
}

pub fn capture_desktop_snapshot(bounds: &SelectionRect) -> anyhow::Result<CachedScreenCapture> {
    if bounds.width < 2 || bounds.height < 2 {
        return Err(anyhow!("desktop bounds are too small"));
    }

    let image = capture_image_in_rect(bounds)?;
    let cg_image = transfer_to_core_graphics(image);
    Ok(CachedScreenCapture::new(bounds.clone(), cg_image))
}

fn capture_image_in_rect(bounds: &SelectionRect) -> anyhow::Result<ScCgImage> {
    let rect = CGRect::new(
        bounds.x as f64,
        bounds.y as f64,
        bounds.width as f64,
        bounds.height as f64,
    );
    SCScreenshotManager::capture_image_in_rect(rect)
        .map_err(|error| anyhow!("ScreenCaptureKit capture failed: {error}"))
}

fn rgba_image_from_sckit(image: ScCgImage) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let width = image.width() as u32;
    let height = image.height() as u32;
    let rgba = image
        .rgba_data()
        .map_err(|error| anyhow!("failed to extract ScreenCaptureKit RGBA data: {error}"))?;
    ImageBuffer::from_vec(width, height, rgba)
        .ok_or_else(|| anyhow!("ScreenCaptureKit RGBA buffer size mismatch"))
}

fn transfer_to_core_graphics(image: ScCgImage) -> CGImage {
    let ptr = image.as_ptr() as *mut core_graphics::sys::CGImage;
    mem::forget(image);
    unsafe { CGImage::from_ptr(ptr) }
}
