export type ViewState = 'general' | 'history' | 'ocr' | 'ai';

export type AppSettings = {
  capture_shortcut: string;
  translate_shortcut: string;
  max_screenshots: number;
  interface_language: string;
  interface_language_set: boolean;
  screenshot_directory: string;
};

export type OcrPayload = {
  imagePath: string;
  sourceText: string;
  ocrDetectedSourceLanguage?: string | null;
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

export type StorageInfo = {
  data_dir: string;
  screenshot_dir: string;
};
