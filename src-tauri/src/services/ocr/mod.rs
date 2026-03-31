mod vision;
mod mock;

pub use mock::MockOcrService;
pub use vision::VisionOcrService;

use crate::models::{OcrRequest, OcrResponse};

pub trait OcrService: Send + Sync {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse>;
}