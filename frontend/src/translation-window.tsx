import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import TranslationWidget from './components/TranslationWidget';
import './index.css';
import { normalizeLanguage, setupI18n } from './i18n/config';
import { TranslationPayload, OcrPayload, AppSettings } from './types';

window.addEventListener('contextmenu', (event) => {
  event.preventDefault();
});

const placeholderPayload: TranslationPayload = {
  imagePath: '',
  sourceText: '',
  translatedText: '',
  provider: '',
  detectedSourceLanguage: null,
  ocrDetectedSourceLanguage: null,
  targetLanguage: 'zh',
};

function WidgetApp() {
  const [payload, setPayload] = useState<TranslationPayload>(placeholderPayload);
  const [isLoading, setIsLoading] = useState(false);
  const [isTranslating, setIsTranslating] = useState(false);

  useEffect(() => {
    console.log('[WIDGET] WidgetApp mounted, setting up event listeners');
    let unlistenOcrLoading: (() => void) | undefined;
    let unlistenOcr: (() => void) | undefined;
    let unlistenTranslation: (() => void) | undefined;

    void listen<{ imagePath: string; loading: boolean }>('ocr-loading', (event) => {
      console.log('[WIDGET] ocr-loading event received');
      setPayload((prev) => ({
        ...prev,
        imagePath: event.payload.imagePath,
        sourceText: '',
        translatedText: '',
        detectedSourceLanguage: null,
        ocrDetectedSourceLanguage: null,
      }));
      setIsLoading(true);
      setIsTranslating(false);
    }).then((dispose) => {
      unlistenOcrLoading = dispose;
    });

    void listen<OcrPayload>('ocr-ready', (event) => {
      console.log('[WIDGET] ocr-ready event received');
      setPayload((prev) => ({
        ...prev,
        imagePath: event.payload.imagePath,
        sourceText: event.payload.sourceText,
        translatedText: '',
        ocrDetectedSourceLanguage: event.payload.ocrDetectedSourceLanguage ?? null,
      }));
      setIsLoading(false);
      setIsTranslating(true);
    }).then((dispose) => {
      unlistenOcr = dispose;
    });

    void listen<TranslationPayload>('translation-ready', (event) => {
      console.log('[WIDGET] translation-ready event received');
      setPayload((prev) => ({
        ...event.payload,
        ocrDetectedSourceLanguage: prev.ocrDetectedSourceLanguage ?? null,
      }));
      setIsLoading(false);
      setIsTranslating(false);
    }).then((dispose) => {
      unlistenTranslation = dispose;
    });

    return () => {
      console.log('[WIDGET] cleaning up event listeners');
      unlistenOcrLoading?.();
      unlistenOcr?.();
      unlistenTranslation?.();
    };
  }, []);

  console.log('[WIDGET] current payload state:', payload);

  return (
    <TranslationWidget
      standalone
      payload={payload}
      isLoading={isLoading}
      isTranslating={isTranslating}
      onClose={() => {
        void getCurrentWindow().close();
      }}
    />
  );
}

async function bootstrap() {
  let initialLanguage = normalizeLanguage(navigator.language);

  try {
    const settings = await invoke<AppSettings>('get_app_settings');
    initialLanguage = normalizeLanguage(settings.interface_language);
  } catch {
    initialLanguage = normalizeLanguage(navigator.language);
  }

  await setupI18n(initialLanguage);

  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <WidgetApp />
    </StrictMode>,
  );
}

void bootstrap();
