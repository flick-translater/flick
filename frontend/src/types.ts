export type ViewState = 'general' | 'history' | 'ocr' | 'ai';

export type ProviderSettings = {
  api_key: string;
  api_base_url: string;
  model: string;
  temperature: number;
  max_tokens: number;
  default_prompt: string;
};

export type AISettings = {
  active_provider: string;
  openai: ProviderSettings;
  anthropic: ProviderSettings;
  openai_compatible: ProviderSettings;
  anthropic_compatible: ProviderSettings;
  ollama: ProviderSettings;
  lmstudio: ProviderSettings;
};

export type AppSettings = {
  capture_shortcut: string;
  translate_shortcut: string;
  selected_translate_shortcut: string;
  autostart_enabled: boolean;
  autostart_configured: boolean;
  max_screenshots: number;
  interface_language: string;
  interface_language_set: boolean;
  screenshot_directory: string;
  ocr_auto_translate: boolean;
  ocr_target_language: string;
  ocr_provider: string;
  tts_provider: string;
  ai: AISettings;
};

export type AutostartStatus = {
  enabled: boolean;
  supported: boolean;
};

export type OcrEngineInfo = {
  id: string;
};

export type TtsEngineInfo = {
  id: string;
};

export type AiTestResult = {
  ok: boolean;
  provider: string;
  protocol: string;
  model: string;
  latency_ms: number;
  message: string;
};

export type OcrPayload = {
  imagePath: string;
  sourceText: string;
  ocrDetectedSourceLanguage?: string | null;
  autoTranslateEnabled?: boolean;
  targetLanguage?: string;
};

export type TranslationPayload = {
  imagePath: string;
  sourceText: string;
  translatedText: string;
  provider: string;
  detectedSourceLanguage?: string | null;
  ocrDetectedSourceLanguage?: string | null;
  targetLanguage: string;
};

export type TranslateResponse = {
  provider: string;
  translated_text: string;
  detected_source_language?: string | null;
};

export type TranslateWindowState = {
  image_path: string;
  source_text: string;
  translated_text: string;
  provider: string;
  detected_source_language?: string | null;
  ocr_detected_source_language?: string | null;
  target_language: string;
  is_loading: boolean;
  is_translating: boolean;
};

export type CaptureRecord = {
  id: string;
  created_at: string;
  width: number;
  height: number;
  path: string;
};

export type CaptureHistory = {
  directory: string;
  items: CaptureRecord[];
};

export type TranslationRecord = {
  id: number;
  created_at: string;
  source_text: string;
  translated_text: string;
  source_language?: string | null;
  target_language: string;
  provider: string;
  image_path?: string | null;
};

export type TranslationHistory = {
  database_path: string;
  items: TranslationRecord[];
};

export type StorageInfo = {
  data_dir: string;
  screenshot_dir: string;
};
