//! Replaceable infrastructure services used by the feature layer.

pub(crate) mod ai;
pub(crate) mod ocr;
pub(crate) mod screen_capture;
mod settings;

pub use ai::TranslationGateway;
pub use ocr::{MockOcrService, OcrService, VisionOcrService};
pub use screen_capture::{CachedScreenCapture, ScreenCaptureService};
pub use settings::SettingsStore;
