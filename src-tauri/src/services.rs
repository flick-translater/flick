use std::{borrow::Cow, path::{Path, PathBuf}};

use anyhow::{Context, anyhow};
use arboard::{Clipboard, ImageData};
use image::{ImageBuffer, Rgba, imageops};
use screenshots::Screen;
use serde::{Serialize, de::DeserializeOwned};

use crate::models::{AppSettings, OcrRequest, OcrResponse, OcrTextBlock, SelectionRect, TranslateRequest, TranslateResponse};

#[derive(Clone)]
pub struct CachedScreenCapture {
    pub display_x: i32,
    pub display_y: i32,
    pub display_width: u32,
    pub display_height: u32,
    pub image: ImageBuffer<Rgba<u8>, Vec<u8>>,
}

#[derive(Default)]
pub struct ScreenCaptureService;

impl ScreenCaptureService {
    pub fn capture_all_screens(&self) -> anyhow::Result<Vec<CachedScreenCapture>> {
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

    pub fn capture_selection(
        &self,
        selection: &SelectionRect,
        cached_screens: &[CachedScreenCapture],
    ) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        if selection.width < 2 || selection.height < 2 {
            return Err(anyhow!("selection is too small"));
        }

        let screen = cached_screens
            .iter()
            .find(|screen| {
                let max_x = screen.display_x + screen.display_width as i32;
                let max_y = screen.display_y + screen.display_height as i32;
                selection.x >= screen.display_x
                    && selection.y >= screen.display_y
                    && selection.x < max_x
                    && selection.y < max_y
            })
            .context("failed to find the cached screen for the current selection")?;

        let local_x = selection.x.saturating_sub(screen.display_x);
        let local_y = selection.y.saturating_sub(screen.display_y);

        let available_width = (screen.display_width as i32 - local_x).max(0) as u32;
        let available_height = (screen.display_height as i32 - local_y).max(0) as u32;

        let logical_width = selection.width.min(available_width);
        let logical_height = selection.height.min(available_height);

        if logical_width == 0 || logical_height == 0 {
            return Err(anyhow!("selection is outside of the active display"));
        }

        let scale_x = screen.image.width() as f64 / screen.display_width.max(1) as f64;
        let scale_y = screen.image.height() as f64 / screen.display_height.max(1) as f64;

        let pixel_x = ((local_x as f64) * scale_x).round().max(0.0) as u32;
        let pixel_y = ((local_y as f64) * scale_y).round().max(0.0) as u32;
        let pixel_width = ((logical_width as f64) * scale_x).round().max(1.0) as u32;
        let pixel_height = ((logical_height as f64) * scale_y).round().max(1.0) as u32;

        let bounded_width = pixel_width.min(screen.image.width().saturating_sub(pixel_x));
        let bounded_height = pixel_height.min(screen.image.height().saturating_sub(pixel_y));

        if bounded_width == 0 || bounded_height == 0 {
            return Err(anyhow!("selection is outside of cached screen bounds"));
        }

        Ok(imageops::crop_imm(
            &screen.image,
            pixel_x,
            pixel_y,
            bounded_width,
            bounded_height,
        )
        .to_image())
    }

    pub fn copy_to_clipboard(
        &self,
        image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    ) -> anyhow::Result<()> {
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

pub trait OcrService: Send + Sync {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse>;
}

pub trait TranslationService: Send + Sync {
    fn translate(&self, request: TranslateRequest) -> anyhow::Result<TranslateResponse>;
}

pub struct MockOcrService;

impl OcrService for MockOcrService {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse> {
        Ok(OcrResponse {
            provider: "mock-ocr".into(),
            text: format!(
                "OCR provider is not configured yet. Placeholder request received for {}.",
                request.image_path
            ),
            blocks: vec![OcrTextBlock {
                text: "Replace MockOcrService with a real OCR engine.".into(),
                confidence: 0.99,
            }],
        })
    }
}

pub struct MockTranslationService;

impl TranslationService for MockTranslationService {
    fn translate(&self, request: TranslateRequest) -> anyhow::Result<TranslateResponse> {
        Ok(TranslateResponse {
            provider: "mock-translate".into(),
            translated_text: format!(
                "[{}] {}",
                request.target_language,
                request.text.trim()
            ),
            detected_source_language: request.source_language.or(Some("auto".into())),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load_settings(&self) -> anyhow::Result<AppSettings> {
        self.load_json().or_else(|error| {
            if self.path.exists() {
                Err(error)
            } else {
                Ok(AppSettings::default())
            }
        })
    }

    pub fn save_settings(&self, settings: &AppSettings) -> anyhow::Result<()> {
        self.save_json(settings)
    }

    fn load_json<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    fn save_json<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(value)?;
        std::fs::write(&self.path, content)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }
}
