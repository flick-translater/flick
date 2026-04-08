use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, anyhow};
use image25::ImageReader;
use oar_ocr::prelude::OAROCRBuilder;

use super::OcrService;
use crate::models::{OcrResponse, OcrTextBlock};

const OCR_MODELS_DIR_ENV: &str = "FLICK_OCR_PADDLE_OCR_V5_MOBILE_MODELS_DIR";
const DET_MODEL_FILE: &str = "paddle_ocr_v5_mobile_text_detection.onnx";
const REC_MODEL_FILE: &str = "paddle_ocr_v5_mobile_text_recognition.onnx";
const DICT_FILE: &str = "paddle_ocr_v5_mobile_characters.txt";
const LINE_ORIENTATION_MODEL_FILE: &str = "text_line_orientation.onnx";

pub struct PaddleOcrV5MobileOcrService {
    bundle: PaddleOcrV5MobileModelBundle,
    runtime: Mutex<Option<PaddleOcrV5MobileRuntime>>,
}

struct PaddleOcrV5MobileRuntime {
    ocr: oar_ocr::prelude::OAROCR,
}

#[derive(Clone, Debug)]
struct PaddleOcrV5MobileModelBundle {
    root: PathBuf,
    detection_model: PathBuf,
    recognition_model: PathBuf,
    dictionary: PathBuf,
    line_orientation_model: Option<PathBuf>,
}

impl PaddleOcrV5MobileOcrService {
    pub fn new(model_dir: &Path) -> Self {
        Self {
            bundle: PaddleOcrV5MobileModelBundle::from_model_dir(model_dir),
            runtime: Mutex::new(None),
        }
    }

    fn runtime(
        &self,
    ) -> anyhow::Result<std::sync::MutexGuard<'_, Option<PaddleOcrV5MobileRuntime>>> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow!("paddle OCRV5 mobile runtime mutex poisoned"))?;

        if runtime.is_none() {
            *runtime = Some(PaddleOcrV5MobileRuntime {
                ocr: self.bundle.build_ocr()?,
            });
        }

        Ok(runtime)
    }
}

impl OcrService for PaddleOcrV5MobileOcrService {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse> {
        let image = ImageReader::new(std::io::Cursor::new(image_data))
            .with_guessed_format()
            .context("failed to detect OCR image format")?
            .decode()
            .context("failed to decode OCR image data")?
            .to_rgb8();

        let mut runtime = self.runtime()?;
        let result = runtime
            .as_mut()
            .expect("paddle OCRV5 mobile runtime initialized")
            .ocr
            .predict(vec![image])
            .context("Paddle OCRV5 Mobile inference failed")?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Paddle OCRV5 Mobile returned no result"))?;

        let blocks: Vec<OcrTextBlock> = result
            .text_regions
            .into_iter()
            .filter_map(|region| {
                let text = region.text?;
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }

                Some(OcrTextBlock {
                    text: trimmed.to_string(),
                    confidence: region.confidence.unwrap_or(0.0),
                })
            })
            .collect();

        let text = blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(OcrResponse {
            provider: "paddle_ocr_v5_mobile".into(),
            text,
            blocks,
        })
    }
}

impl PaddleOcrV5MobileModelBundle {
    fn from_model_dir(model_dir: &Path) -> Self {
        let root = std::env::var_os(OCR_MODELS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| model_dir.to_path_buf());
        let line_orientation_model = root.join(LINE_ORIENTATION_MODEL_FILE);

        Self {
            detection_model: root.join(DET_MODEL_FILE),
            recognition_model: root.join(REC_MODEL_FILE),
            dictionary: root.join(DICT_FILE),
            line_orientation_model: line_orientation_model
                .is_file()
                .then_some(line_orientation_model),
            root,
        }
    }

    fn build_ocr(&self) -> anyhow::Result<oar_ocr::prelude::OAROCR> {
        self.ensure_required_files()?;

        let mut builder = OAROCRBuilder::new(
            &self.detection_model,
            &self.recognition_model,
            &self.dictionary,
        )
        .image_batch_size(1)
        .region_batch_size(32);

        if let Some(model) = &self.line_orientation_model {
            builder = builder.with_text_line_orientation_classification(model);
        }

        builder.build().map_err(|error| {
            anyhow!(
                "failed to initialize Paddle OCRV5 Mobile from {}: {error}",
                self.root.display()
            )
        })
    }

    fn ensure_required_files(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create Paddle OCRV5 Mobile model directory at {}",
                self.root.display()
            )
        })?;

        for path in [
            &self.detection_model,
            &self.recognition_model,
            &self.dictionary,
        ] {
            if !path.is_file() {
                return Err(anyhow!(
                    "missing Paddle OCRV5 Mobile asset: {}. Expected files under {}",
                    path.display(),
                    self.root.display()
                ));
            }
        }

        Ok(())
    }
}
