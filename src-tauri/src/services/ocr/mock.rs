use super::OcrService;
use crate::models::{OcrRequest, OcrResponse, OcrTextBlock};

pub struct MockOcrService;

impl OcrService for MockOcrService {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse> {
        Ok(OcrResponse {
            provider: "mock-ocr".into(),
            text: format!(
                "OCR provider is not configured yet. Placeholder request received for {}.",
                request.image_path
            ),
            blocks: vec![OcrTextBlock {
                text: "Replace MockOcrService with a real OCR engine.".into(),
                confidence: 0.99,
            }],
        })
    }

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
