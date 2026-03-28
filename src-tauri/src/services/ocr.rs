use crate::models::{OcrRequest, OcrResponse, OcrTextBlock};

pub trait OcrService: Send + Sync {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse>;
}

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
}
