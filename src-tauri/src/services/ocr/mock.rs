use super::OcrService;
use crate::models::{OcrResponse, OcrTextBlock};

pub struct MockOcrService;

impl OcrService for MockOcrService {
    fn run_with_data(&self, _image_data: &[u8]) -> anyhow::Result<OcrResponse> {
        Ok(OcrResponse {
            provider: "mock-ocr".into(),
            text: "OCR from data: mock result".into(),
            blocks: vec![OcrTextBlock {
                text: "Mock OCR from image data.".into(),
                confidence: 0.99,
            }],
        })
    }
}
