//! Replaceable infrastructure services used by the feature layer.

pub(crate) mod ai;
pub(crate) mod ocr;
pub(crate) mod screen_capture;
mod settings;

pub use ai::TranslationGateway;
pub use ocr::{OcrService, available_ocr_engines, create_ocr_service, default_ocr_provider};
pub use screen_capture::{CachedScreenCapture, ScreenCaptureService};
pub use settings::SettingsStore;
