use std::path::Path;
use std::process::Command;

use anyhow::anyhow;

use super::OcrService;
use crate::models::{OcrRequest, OcrResponse, OcrTextBlock};

pub struct VisionOcrService;

impl OcrService for VisionOcrService {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse> {
        let path = Path::new(&request.image_path);
        if !path.exists() {
            return Err(anyhow!("image file not found: {}", request.image_path));
        }

        let text = recognize_text(&request.image_path)?;

        Ok(OcrResponse {
            provider: "vision".into(),
            text: text.clone(),
            blocks: vec![OcrTextBlock {
                text,
                confidence: 1.0,
            }],
        })
    }
}

#[cfg(target_os = "macos")]
fn recognize_text(image_path: &str) -> anyhow::Result<String> {
    let script = format!(
        r#"
import Vision
import Foundation

let imagePath = "{path}"
let url = URL(fileURLWithPath: imagePath)

guard let imageData = try? Data(contentsOf: url),
      let cgImageSource = CGImageSourceCreateWithData(imageData as CFData, nil),
      let cgImage = CGImageSourceCreateImageAtIndex(cgImageSource, 0, nil) else {{
    print("")
    exit(0)
}}

let requestHandler = VNImageRequestHandler(cgImage: cgImage, options: [:])
let textRequest = VNRecognizeTextRequest()
textRequest.recognitionLevel = .accurate
textRequest.recognitionLanguages = ["zh-Hans", "zh-Hant", "en", "ja"]

do {{
    try requestHandler.perform([textRequest])
}} catch {{
    print("")
    exit(0)
}}

guard let observations = textRequest.results else {{
    print("")
    exit(0)
}}

var texts: [String] = []
for observation in observations {{
    if let candidate = observation.topCandidates(1).first {{
        texts.append(candidate.string)
    }}
}}
print(texts.joined(separator: "\n"))
"#,
        path = image_path.replace("\\", "\\\\").replace("\"", "\\\"")
    );

    let output = Command::new("swift")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| anyhow!("failed to execute swift: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = stdout.trim().to_string();

    Ok(text)
}

#[cfg(not(target_os = "macos"))]
fn recognize_text(_image_path: &str) -> anyhow::Result<String> {
    Err(anyhow!("Vision OCR is only available on macOS"))
}
