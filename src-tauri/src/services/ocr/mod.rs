#[cfg(target_os = "macos")]
mod macos_vision;
mod mock;
#[cfg(any(
    target_os = "linux",
    target_os = "windows",
    all(target_os = "macos", target_arch = "aarch64")
))]
mod paddle_ocr_v5_mobile;
#[cfg(target_os = "windows")]
mod windows_builtin;

use std::{path::Path, sync::Arc};

#[cfg(target_os = "macos")]
pub use macos_vision::MacosVisionOcrService;
pub use mock::MockOcrService;
#[cfg(any(
    target_os = "linux",
    target_os = "windows",
    all(target_os = "macos", target_arch = "aarch64")
))]
pub use paddle_ocr_v5_mobile::PaddleOcrV5MobileOcrService;
#[cfg(target_os = "windows")]
pub use windows_builtin::WindowsBuiltinOcrService;

use crate::models::{OcrEngineInfo, OcrResponse};

pub const PADDLE_OCR_V5_MOBILE_ID: &str = "paddle_ocr_v5_mobile";

pub trait OcrService: Send + Sync {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse>;
}

pub fn normalize_ocr_engine_id(engine_id: &str) -> String {
    match engine_id.trim().to_lowercase().as_str() {
        "onnx" | "paddle-ocr-v5-mobile" | "paddle_ocr_v5_mobile" => {
            PADDLE_OCR_V5_MOBILE_ID.into()
        }
        other => other.to_string(),
    }
}

pub fn create_ocr_service(engine_id: &str, model_dir: &Path) -> Arc<dyn OcrService> {
    let engine_id = normalize_ocr_engine_id(engine_id);

    #[cfg(target_os = "macos")]
    {
        match engine_id.as_str() {
            "mock" => Arc::new(MockOcrService),
            #[cfg(target_arch = "aarch64")]
            PADDLE_OCR_V5_MOBILE_ID => Arc::new(PaddleOcrV5MobileOcrService::new(model_dir)),
            _ => Arc::new(MacosVisionOcrService),
        }
    }

    #[cfg(target_os = "linux")]
    {
        match engine_id.as_str() {
            "mock" => Arc::new(MockOcrService),
            _ => Arc::new(PaddleOcrV5MobileOcrService::new(model_dir)),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match engine_id.as_str() {
            "mock" => Arc::new(MockOcrService),
            "windows" => Arc::new(WindowsBuiltinOcrService),
            _ => Arc::new(PaddleOcrV5MobileOcrService::new(model_dir)),
        }
    }
}

pub fn available_ocr_engines() -> Vec<OcrEngineInfo> {
    #[cfg(target_os = "macos")]
    {
        let engines = vec![OcrEngineInfo {
            id: "vision".into(),
        }];
        #[cfg(target_arch = "aarch64")]
        {
            let mut engines = engines;
            engines.push(OcrEngineInfo {
                id: PADDLE_OCR_V5_MOBILE_ID.into(),
            });
            return engines;
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            engines
        }
    }

    #[cfg(target_os = "linux")]
    {
        vec![OcrEngineInfo {
            id: PADDLE_OCR_V5_MOBILE_ID.into(),
        }]
    }

    #[cfg(target_os = "windows")]
    {
        vec![
            OcrEngineInfo {
                id: "windows".into(),
            },
            OcrEngineInfo {
                id: PADDLE_OCR_V5_MOBILE_ID.into(),
            },
        ]
    }
}

pub fn default_ocr_provider() -> String {
    #[cfg(target_os = "macos")]
    {
        "vision".into()
    }

    #[cfg(target_os = "linux")]
    {
        PADDLE_OCR_V5_MOBILE_ID.into()
    }

    #[cfg(target_os = "windows")]
    {
        "windows".into()
    }
}
