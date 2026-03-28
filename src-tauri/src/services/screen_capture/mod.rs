#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod non_macos;

use std::{borrow::Cow, path::Path};

use anyhow::Context;
use arboard::{Clipboard, ImageData};
use image::{ImageBuffer, Rgba};

use crate::models::SelectionRect;

#[cfg(not(target_os = "macos"))]
use screenshots::Screen;

#[cfg(not(target_os = "macos"))]
#[derive(Clone)]
pub struct CachedScreenCapture {
    pub display_x: i32,
    pub display_y: i32,
    pub display_width: u32,
    pub display_height: u32,
    pub image: ImageBuffer<Rgba<u8>, Vec<u8>>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Default)]
pub struct CachedScreenCapture;

#[derive(Default)]
pub struct ScreenCaptureService;

#[cfg_attr(target_os = "macos", allow(dead_code))]
impl ScreenCaptureService {
    pub fn capture_all_screens(&self) -> anyhow::Result<Vec<CachedScreenCapture>> {
        #[cfg(target_os = "macos")]
        {
            Ok(Vec::new())
        }

        #[cfg(not(target_os = "macos"))]
        {
            Screen::all()?
                .into_iter()
                .map(|screen| {
                    let display = screen.display_info;
                    let image = screen
                        .capture()
                        .context("failed to capture screen snapshot")?;

                    Ok(CachedScreenCapture {
                        display_x: display.x,
                        display_y: display.y,
                        display_width: display.width,
                        display_height: display.height,
                        image,
                    })
                })
                .collect()
        }
    }

    pub fn capture_selection(
        &self,
        selection: &SelectionRect,
        cached_screens: &[CachedScreenCapture],
    ) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        #[cfg(target_os = "macos")]
        {
            let _ = cached_screens;
            macos::capture_selection(selection)
        }

        #[cfg(not(target_os = "macos"))]
        {
            non_macos::capture_selection(selection, cached_screens)
        }
    }

    pub fn copy_to_clipboard(&self, image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> anyhow::Result<()> {
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
