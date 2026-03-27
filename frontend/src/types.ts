export type ViewState = 'general' | 'history' | 'ocr' | 'ai';

export type AppSettings = {
  capture_shortcut: string;
  translate_shortcut: string;
};

export type TranslationPayload = {
  imagePath: string;
  sourceText: string;
  translatedText: string;
  provider: string;
  detectedSourceLanguage?: string | null;
  targetLanguage: string;
};
