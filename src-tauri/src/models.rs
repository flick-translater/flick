use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureContext {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub type CaptureContexts = HashMap<String, CaptureContext>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRecord {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub width: u32,
    pub height: u32,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureHistory {
    pub directory: String,
    pub items: Vec<CaptureRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub data_dir: String,
    pub screenshot_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutostartStatus {
    pub enabled: bool,
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrRequest {
    pub image_path: String,
    pub language_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResponse {
    pub provider: String,
    pub text: String,
    pub blocks: Vec<OcrTextBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrTextBlock {
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateRequest {
    pub text: String,
    pub source_language: Option<String>,
    pub target_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateResponse {
    pub provider: String,
    pub translated_text: String,
    pub detected_source_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub capture_shortcut: String,
    pub translate_shortcut: String,
    pub max_screenshots: u32,
    pub interface_language: String,
    pub interface_language_set: bool,
    pub screenshot_directory: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            capture_shortcut: "CommandOrControl+Alt+A".into(),
            translate_shortcut: "CommandOrControl+Alt+T".into(),
            max_screenshots: 500,
            interface_language: "en".into(),
            interface_language_set: false,
            screenshot_directory: String::new(),
        }
    }
}
