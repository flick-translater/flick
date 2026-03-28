//! Replaceable infrastructure services used by the feature layer.

mod ocr;
pub(crate) mod screen_capture;
mod settings;
mod translation;

pub use ocr::{MockOcrService, OcrService};
pub use screen_capture::{CachedScreenCapture, ScreenCaptureService};
pub use settings::SettingsStore;
pub use translation::{MockTranslationService, TranslationService};
