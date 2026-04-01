mod mock;
mod vision;

pub use mock::MockOcrService;
pub use vision::VisionOcrService;

use crate::models::OcrResponse;

pub trait OcrService: Send + Sync {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse>;
}
