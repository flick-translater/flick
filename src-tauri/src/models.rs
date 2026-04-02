//! Serializable request/response and state models shared across the backend and frontend.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
pub struct TranslationRecord {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub source_text: String,
    pub translated_text: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub provider: String,
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationHistory {
    pub database_path: String,
    pub items: Vec<TranslationRecord>,
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
pub struct OcrEngineInfo {
    pub id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranslateWindowState {
    pub image_path: String,
    pub source_text: String,
    pub translated_text: String,
    pub provider: String,
    pub detected_source_language: Option<String>,
    pub ocr_detected_source_language: Option<String>,
    pub target_language: String,
    pub is_loading: bool,
    pub is_translating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTestResult {
    pub ok: bool,
    pub provider: String,
    pub protocol: String,
    pub model: String,
    pub latency_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderSettings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_openai_api_base_url")]
    pub api_base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_prompt")]
    pub default_prompt: String,
}

fn default_openai_api_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_model() -> String {
    "gpt-4o-mini".into()
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    0
}

fn default_prompt() -> String {
    "You are a professional translator. Translate the following text accurately while preserving the original meaning and tone. Only output the translated text, nothing else.".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AISettings {
    #[serde(default = "default_provider")]
    pub active_provider: String,
    #[serde(default)]
    pub openai: ProviderSettings,
    #[serde(default)]
    pub anthropic: ProviderSettings,
    #[serde(default)]
    pub openai_compatible: ProviderSettings,
    #[serde(default)]
    pub anthropic_compatible: ProviderSettings,
    #[serde(default)]
    pub ollama: ProviderSettings,
    #[serde(default)]
    pub lmstudio: ProviderSettings,
}

fn default_provider() -> String {
    "openai".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub capture_shortcut: String,
    pub translate_shortcut: String,
    pub selected_translate_shortcut: String,
    pub autostart_enabled: bool,
    pub autostart_configured: bool,
    pub max_screenshots: u32,
    pub interface_language: String,
    pub interface_language_set: bool,
    pub screenshot_directory: String,
    pub ocr_shortcut_enabled: bool,
    pub ocr_auto_translate: bool,
    pub ocr_target_language: String,
    pub ocr_provider: String,
    pub ai: AISettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            capture_shortcut: "CommandOrControl+Alt+A".into(),
            translate_shortcut: "CommandOrControl+Alt+T".into(),
            selected_translate_shortcut: "CommandOrControl+Alt+S".into(),
            autostart_enabled: false,
            autostart_configured: false,
            max_screenshots: 500,
            interface_language: "en".into(),
            interface_language_set: false,
            screenshot_directory: String::new(),
            ocr_shortcut_enabled: true,
            ocr_auto_translate: true,
            ocr_target_language: "zh".into(),
            ocr_provider: "vision".into(),
            ai: AISettings::default(),
        }
    }
}
