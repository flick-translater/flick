import { StrictMode, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import TranslationWindow from './components/TranslationWindow';
import './index.css';
import { normalizeLanguage, setupI18n } from './i18n/config';
import { TranslationPayload, AppSettings, TranslateResponse, TranslateWindowState } from './types';

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

function buildPayloadFromSnapshot(snapshot: TranslateWindowState): TranslationPayload {
  return {
    imagePath: snapshot.image_path,
    sourceText: snapshot.source_text,
    translatedText: snapshot.translated_text,
    provider: snapshot.provider,
    detectedSourceLanguage: snapshot.detected_source_language ?? null,
    ocrDetectedSourceLanguage: snapshot.ocr_detected_source_language ?? null,
    targetLanguage: snapshot.target_language || placeholderPayload.targetLanguage,
  };
}

function TranslateWindowApp() {
  const [payload, setPayload] = useState<TranslationPayload>(placeholderPayload);
  const [isLoading, setIsLoading] = useState(false);
  const [isTranslating, setIsTranslating] = useState(false);
  const lastSnapshotRef = useRef('');

  const syncFromSnapshot = async () => {
    try {
      const snapshot = await invoke<TranslateWindowState>('get_translate_window_state');
      const nextKey = JSON.stringify(snapshot);

      if (nextKey === lastSnapshotRef.current) {
        return;
      }

      lastSnapshotRef.current = nextKey;
      setPayload(buildPayloadFromSnapshot(snapshot));
      setIsLoading(snapshot.is_loading);
      setIsTranslating(snapshot.is_translating);
    } catch (error) {
      console.error('Failed to read translate window state', error);
    }
  };

  const handleTranslate = async () => {
    if (!payload.sourceText.trim()) {
      return;
    }

    setIsTranslating(true);
    setPayload((prev) => ({
      ...prev,
      translatedText: '',
    }));

    try {
      const response = await invoke<TranslateResponse>('translate', {
        request: {
          text: payload.sourceText,
          source_language: payload.ocrDetectedSourceLanguage ?? null,
          target_language: payload.targetLanguage,
        },
      });

      setPayload((prev) => ({
        ...prev,
        translatedText: response.translated_text,
        provider: response.provider,
        detectedSourceLanguage: response.detected_source_language ?? prev.detectedSourceLanguage,
      }));
    } catch (error) {
      console.error('[TRANSLATE] manual translate failed', error);
    } finally {
      setIsTranslating(false);
    }
  };

  useEffect(() => {
    void syncFromSnapshot();
    const intervalId = window.setInterval(() => {
      void syncFromSnapshot();
    }, 250);

    return () => {
      window.clearInterval(intervalId);
    };
  }, []);

  return (
    <TranslationWindow
      standalone
      payload={payload}
      isLoading={isLoading}
      isTranslating={isTranslating}
      onTranslate={() => {
        void handleTranslate();
      }}
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
      <TranslateWindowApp />
    </StrictMode>,
  );
}

void bootstrap();
