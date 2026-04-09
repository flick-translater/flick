//! Cross-platform screenshot service facade.
//!
//! The feature layer talks only to this facade; platform-specific capture code stays behind
//! conditional modules so the outer workflow does not need OS branching everywhere.

#[cfg(target_os = "linux")]
mod linux_platform;
#[cfg(target_os = "macos")]
pub(crate) mod macos_frozen_platform;
#[cfg(target_os = "macos")]
mod macos_platform;
#[cfg(target_os = "macos")]
mod macos_screen_capture_kit_platform;
#[cfg(target_os = "windows")]
mod windows_platform;

use std::{path::Path, sync::Arc};

use anyhow::Context;
#[cfg(not(target_os = "macos"))]
use arboard::{Clipboard, ImageData};
#[cfg(target_os = "linux")]
use arboard::{LinuxClipboardKind, SetExtLinux};
#[cfg(target_os = "linux")]
use std::borrow::Cow;
#[cfg(target_os = "macos")]
use core_graphics::image::CGImage;
use image::{ImageBuffer, Rgba};
#[cfg(target_os = "macos")]
use image::ImageEncoder;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
#[cfg(target_os = "macos")]
use objc2_foundation::NSData;

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
    #[cfg(target_os = "linux")]
    pub image: Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    #[cfg(target_os = "windows")]
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

    #[cfg(target_os = "linux")]
    pub fn new(bounds: SelectionRect, image: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Self {
        Self {
            bounds,
            image: Arc::new(image),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn new(bounds: SelectionRect, image: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Self {
        Self {
            bounds,
            image: Arc::new(image),
        }
    }
}

#[cfg(target_os = "macos")]
trait MacosCaptureBackend: Sync {
    fn name(&self) -> &'static str;
    fn capture_selection(
        &self,
        selection: &SelectionRect,
    ) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>>;
    fn capture_desktop_snapshot(
        &self,
        bounds: &SelectionRect,
    ) -> anyhow::Result<CachedScreenCapture>;
}

#[cfg(target_os = "macos")]
fn preferred_macos_capture_backend() -> &'static dyn MacosCaptureBackend {
    &macos_screen_capture_kit_platform::ScreenCaptureKitBackend
}

#[cfg(target_os = "macos")]
fn fallback_macos_capture_backend() -> &'static dyn MacosCaptureBackend {
    &macos_platform::CoreGraphicsCaptureBackend
}

#[cfg(target_os = "macos")]
fn capture_selection_via_backend(
    selection: &SelectionRect,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let preferred = preferred_macos_capture_backend();
    match preferred.capture_selection(selection) {
        Ok(image) => Ok(image),
        Err(error) => {
            eprintln!(
                "{} capture failed, falling back to {}: {error}",
                preferred.name(),
                fallback_macos_capture_backend().name()
            );
            fallback_macos_capture_backend()
                .capture_selection(selection)
                .with_context(|| {
                    format!(
                        "{} capture failed before Core Graphics fallback: {error}",
                        preferred.name()
                    )
                })
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_desktop_snapshot_via_backend(
    bounds: &SelectionRect,
) -> anyhow::Result<CachedScreenCapture> {
    let preferred = preferred_macos_capture_backend();
    match preferred.capture_desktop_snapshot(bounds) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            eprintln!(
                "{} desktop snapshot failed, falling back to {}: {error}",
                preferred.name(),
                fallback_macos_capture_backend().name()
            );
            fallback_macos_capture_backend()
                .capture_desktop_snapshot(bounds)
                .with_context(|| {
                    format!(
                        "{} desktop snapshot failed before Core Graphics fallback: {error}",
                        preferred.name()
                    )
                })
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
        #[cfg(target_os = "macos")]
        {
            if !cached_screens.is_empty() {
                return macos_frozen_platform::capture_from_snapshot(selection, cached_screens);
            }

            return capture_selection_via_backend(selection);
        }

        #[cfg(target_os = "linux")]
        {
            return linux_platform::capture_selection(selection, cached_screens);
        }

        #[cfg(target_os = "windows")]
        {
            return windows_platform::capture_selection(selection, cached_screens);
        }
    }

    pub fn copy_to_clipboard(&self, image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            return copy_to_clipboard_linux(image);
        }

        #[cfg(target_os = "macos")]
        {
            return copy_to_clipboard_macos(image);
        }

        #[cfg(all(not(target_os = "linux"), not(target_os = "macos")))]
        {
            let mut clipboard = Clipboard::new().context("failed to access clipboard")?;
            let width = usize::try_from(image.width()).context("invalid image width")?;
            let height = usize::try_from(image.height()).context("invalid image height")?;

            clipboard
                .set_image(ImageData {
                    width,
                    height,
                    bytes: Cow::Borrowed(image.as_raw()),
                })
                .context("failed to write screenshot to clipboard")?;
            Ok(())
        }
    }

    pub fn save_png(
        &self,
        image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
        path: &Path,
    ) -> anyhow::Result<()> {
        image.save(path).context("failed to save screenshot")?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn copy_to_clipboard_linux(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> anyhow::Result<()> {
    let width = usize::try_from(image.width()).context("invalid image width")?;
    let height = usize::try_from(image.height()).context("invalid image height")?;
    let bytes = image.as_raw().clone();

    std::thread::spawn(move || {
        let run = || -> anyhow::Result<()> {
            let mut clipboard = Clipboard::new().context("failed to access clipboard")?;
            clipboard
                .set()
                .clipboard(LinuxClipboardKind::Clipboard)
                .wait()
                .image(ImageData {
                    width,
                    height,
                    bytes: Cow::Owned(bytes),
                })
                .context("failed to write screenshot to clipboard")?;
            Ok(())
        };

        if let Err(_error) = run() {
            // Ignore background clipboard ownership failures silently in production.
        }
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_to_clipboard_macos(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> anyhow::Result<()> {
    let mut png_bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png_bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ColorType::Rgba8.into(),
        )
        .context("failed to encode screenshot as PNG")?;

    let data = NSData::with_bytes(&png_bytes);
    let pasteboard = NSPasteboard::generalPasteboard();
    let mut last_error = None;

    for _ in 0..3 {
        pasteboard.clearContents();
        if pasteboard.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG }) {
            return Ok(());
        }

        last_error = Some(anyhow::anyhow!("NSPasteboard rejected PNG payload"));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("failed to write screenshot to clipboard")))
}
