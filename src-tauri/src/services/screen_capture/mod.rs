//! Cross-platform screenshot service facade.
//!
//! The feature layer talks only to this facade; platform-specific capture code stays behind
//! conditional modules so the outer workflow does not need OS branching everywhere.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod macos_frozen;

use std::{borrow::Cow, path::Path, sync::Arc};

use anyhow::Context;
use arboard::{Clipboard, ImageData};
#[cfg(target_os = "macos")]
use core_graphics::image::CGImage;
use image::{ImageBuffer, Rgba};

use crate::models::SelectionRect;

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct CachedCgImage(pub CGImage);

#[cfg(target_os = "macos")]
unsafe impl Send for CachedCgImage {}

#[cfg(target_os = "macos")]
unsafe impl Sync for CachedCgImage {}

#[derive(Clone)]
pub struct CachedScreenCapture {
    pub bounds: SelectionRect,
    #[cfg(target_os = "macos")]
    pub image: Arc<CachedCgImage>,
    #[cfg(not(target_os = "macos"))]
    pub image: Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>,
}

impl CachedScreenCapture {
    #[cfg(target_os = "macos")]
    pub fn new(bounds: SelectionRect, image: CGImage) -> Self {
        Self {
            bounds,
            image: Arc::new(CachedCgImage(image)),
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn new(bounds: SelectionRect, image: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Self {
        Self {
            bounds,
            image: Arc::new(image),
        }
    }
}

#[derive(Default)]
pub struct ScreenCaptureService;

impl ScreenCaptureService {
    pub fn capture_selection(
        &self,
        selection: &SelectionRect,
        cached_screens: &[CachedScreenCapture],
    ) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        if cfg!(target_os = "macos") {
            #[cfg(target_os = "macos")]
            {
                if !cached_screens.is_empty() {
                    return macos_frozen::capture_from_snapshot(selection, cached_screens);
                }

                return macos::capture_selection(selection);
            }
        }

        let _ = cached_screens;
        anyhow::bail!("capture is not implemented on this platform")
    }

    pub fn copy_to_clipboard(&self, image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> anyhow::Result<()> {
        // Clipboard integration is shared, so it stays in the facade instead of per-platform code.
        let mut clipboard = Clipboard::new().context("failed to access clipboard")?;
        let width = usize::try_from(image.width()).context("invalid image width")?;
        let height = usize::try_from(image.height()).context("invalid image height")?;

        clipboard
            .set_image(ImageData {
                width,
                height,
                bytes: Cow::Borrowed(image.as_raw()),
            })
            .context("failed to write screenshot to clipboard")
    }

    pub fn save_png(
        &self,
        image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
        path: &Path,
    ) -> anyhow::Result<()> {
        image.save(path).context("failed to save screenshot")
    }
}
