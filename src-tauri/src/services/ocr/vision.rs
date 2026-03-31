use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::anyhow;
use once_cell::sync::Lazy;

use super::OcrService;
use crate::models::{OcrRequest, OcrResponse, OcrTextBlock};

const CACHE_TTL_SECS: u64 = 300;
const MAX_CACHE_SIZE: usize = 100;

struct CacheEntry {
    text: String,
    timestamp: Instant,
}

static OCR_CACHE: Lazy<Arc<Mutex<HashMap<String, CacheEntry>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub struct VisionOcrService;

impl OcrService for VisionOcrService {
    fn run(&self, request: OcrRequest) -> anyhow::Result<OcrResponse> {
        let path = Path::new(&request.image_path);
        if !path.exists() {
            return Err(anyhow!("image file not found: {}", request.image_path));
        }

        let cache_key = generate_cache_key(&request.image_path)?;

        if let Some(cached) = check_cache(&cache_key) {
            return Ok(OcrResponse {
                provider: "vision".into(),
                text: cached.clone(),
                blocks: vec![OcrTextBlock {
                    text: cached,
                    confidence: 1.0,
                }],
            });
        }

        let text = recognize_text(&request.image_path)?;

        update_cache(cache_key, text.clone());

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

fn generate_cache_key(image_path: &str) -> anyhow::Result<String> {
    let metadata = std::fs::metadata(image_path)?;
    let modified = metadata.modified()?;
    let size = metadata.len();
    Ok(format!(
        "{}:{}:{}",
        image_path,
        modified.duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        size
    ))
}

fn check_cache(cache_key: &str) -> Option<String> {
    let cache = OCR_CACHE.lock().ok()?;
    let entry = cache.get(cache_key)?;

    if entry.timestamp.elapsed().as_secs() < CACHE_TTL_SECS {
        Some(entry.text.clone())
    } else {
        None
    }
}

fn update_cache(cache_key: String, text: String) {
    if let Ok(mut cache) = OCR_CACHE.lock() {
        if cache.len() >= MAX_CACHE_SIZE {
            let now = Instant::now();
            cache.retain(|_, entry| now.duration_since(entry.timestamp).as_secs() < CACHE_TTL_SECS);

            if cache.len() >= MAX_CACHE_SIZE {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.timestamp)
                    .map(|(k, _)| k.clone());
                if let Some(key) = oldest {
                    cache.remove(&key);
                }
            }
        }

        cache.insert(
            cache_key,
            CacheEntry {
                text,
                timestamp: Instant::now(),
            },
        );
    }
}

#[cfg(target_os = "macos")]
fn recognize_text(image_path: &str) -> anyhow::Result<String> {
    if let Ok(ocr_tool) = get_ocr_tool_path() {
        if ocr_tool.exists() {
            return recognize_text_with_tool(&ocr_tool, image_path);
        }
    }

    recognize_text_with_swift(image_path)
}

#[cfg(target_os = "macos")]
fn recognize_text_with_tool(
    ocr_tool: &std::path::Path,
    image_path: &str,
) -> anyhow::Result<String> {
    let output = Command::new(ocr_tool)
        .arg(image_path)
        .output()
        .map_err(|e| anyhow!("failed to execute ocr-tool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ocr-tool failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

#[cfg(target_os = "macos")]
fn recognize_text_with_swift(image_path: &str) -> anyhow::Result<String> {
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
        path = image_path.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let output = Command::new("swift")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| anyhow!("failed to execute swift: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

#[cfg(target_os = "macos")]
fn get_ocr_tool_path() -> anyhow::Result<std::path::PathBuf> {
    if let Ok(path) = std::env::var("OCR_TOOL_PATH") {
        return Ok(std::path::PathBuf::from(path));
    }

    let current_exe = std::env::current_exe()?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("cannot get exe directory"))?;

    let possible_paths = vec![
        exe_dir.join("ocr-tool"),
        exe_dir.join("../Resources/ocr-tool"),
        exe_dir.join("../../Resources/ocr-tool"),
    ];

    for path in possible_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    Ok(which::which("ocr-tool").map_err(|_| anyhow!("ocr-tool not found"))?)
}

#[cfg(not(target_os = "macos"))]
fn recognize_text(_image_path: &str) -> anyhow::Result<String> {
    Err(anyhow!("Vision OCR is only available on macOS"))
}
