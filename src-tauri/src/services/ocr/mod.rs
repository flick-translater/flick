#[cfg(target_os = "macos")]
mod macos_vision;
mod mock;

use std::sync::Arc;

#[cfg(target_os = "macos")]
pub use macos_vision::MacosVisionOcrService;
pub use mock::MockOcrService;

use crate::models::{OcrEngineInfo, OcrResponse};

pub trait OcrService: Send + Sync {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse>;
}

pub fn create_ocr_service(engine_id: &str) -> Arc<dyn OcrService> {
    #[cfg(target_os = "macos")]
    {
        match engine_id {
            "mock" => Arc::new(MockOcrService),
            _ => Arc::new(MacosVisionOcrService),
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = engine_id;
        Arc::new(MockOcrService)
    }

    #[cfg(target_os = "windows")]
    {
        let _ = engine_id;
        Arc::new(MockOcrService)
    }
}

pub fn available_ocr_engines() -> Vec<OcrEngineInfo> {
    #[cfg(target_os = "macos")]
    {
        vec![OcrEngineInfo {
            id: "vision".into(),
        }]
    }

    #[cfg(target_os = "linux")]
    {
        vec![]
    }

    #[cfg(target_os = "windows")]
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
