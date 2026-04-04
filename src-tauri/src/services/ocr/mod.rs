#[cfg(target_os = "macos")]
mod macos_vision;
mod mock;
mod onnx;

use std::{path::Path, sync::Arc};

#[cfg(target_os = "macos")]
pub use macos_vision::MacosVisionOcrService;
pub use mock::MockOcrService;
pub use onnx::OnnxRuntimeOcrService;

use crate::models::{OcrEngineInfo, OcrResponse};

pub trait OcrService: Send + Sync {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse>;
}

pub fn create_ocr_service(engine_id: &str, model_dir: &Path) -> Arc<dyn OcrService> {
    #[cfg(target_os = "macos")]
    {
        match engine_id {
            "mock" => Arc::new(MockOcrService),
            "onnx" => Arc::new(OnnxRuntimeOcrService::new(model_dir)),
            _ => Arc::new(MacosVisionOcrService),
        }
    }

    #[cfg(target_os = "linux")]
    {
        match engine_id {
            "mock" => Arc::new(MockOcrService),
            _ => Arc::new(OnnxRuntimeOcrService::new(model_dir)),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match engine_id {
            "mock" => Arc::new(MockOcrService),
            _ => Arc::new(OnnxRuntimeOcrService::new(model_dir)),
        }
    }
}

pub fn available_ocr_engines() -> Vec<OcrEngineInfo> {
    #[cfg(target_os = "macos")]
    {
        vec![
            OcrEngineInfo {
                id: "vision".into(),
            },
            OcrEngineInfo { id: "onnx".into() },
        ]
    }

    #[cfg(target_os = "linux")]
    {
        vec![OcrEngineInfo { id: "onnx".into() }]
    }

    #[cfg(target_os = "windows")]
    {
        vec![OcrEngineInfo { id: "onnx".into() }]
    }
}

pub fn default_ocr_provider() -> String {
    #[cfg(target_os = "macos")]
    {
        "vision".into()
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        "onnx".into()
    }
}
