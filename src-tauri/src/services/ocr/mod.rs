mod mock;
mod vision;

use std::sync::Arc;

pub use mock::MockOcrService;
pub use vision::VisionOcrService;

use crate::models::{OcrEngineInfo, OcrResponse};

pub trait OcrService: Send + Sync {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse>;
}

pub fn create_ocr_service(engine_id: &str) -> Arc<dyn OcrService> {
    match engine_id {
        "mock" => Arc::new(MockOcrService),
        _ => Arc::new(VisionOcrService),
    }
}

pub fn available_ocr_engines() -> Vec<OcrEngineInfo> {
    #[cfg(target_os = "macos")]
    {
        vec![OcrEngineInfo {
            id: "vision".into(),
        }]
    }

    #[cfg(not(target_os = "macos"))]
    {
        vec![]
    }
}

pub fn default_ocr_provider() -> String {
    available_ocr_engines()
        .into_iter()
        .next()
        .map(|engine| engine.id)
        .unwrap_or_else(|| "vision".into())
}
