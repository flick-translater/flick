//! Replaceable infrastructure services used by the feature layer.

pub(crate) mod ai;
pub(crate) mod ocr;
pub(crate) mod screen_capture;
mod selected_text;
mod settings;
mod translation_history;
mod tts;

pub use ai::TranslationGateway;
pub use ocr::{
    OcrService, available_ocr_engines, create_ocr_service, default_ocr_provider,
    normalize_ocr_engine_id,
};
pub use screen_capture::{CachedScreenCapture, ScreenCaptureService};
pub use selected_text::read_selected_text;
pub use settings::SettingsStore;
pub use translation_history::{NewTranslationRecord, TranslationHistoryStore};
pub use tts::{TtsService, TtsSnapshot, TtsTarget, available_tts_engines, normalize_tts_engine_id};
