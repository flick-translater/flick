import { StrictMode, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import TranslationWindow from './components/TranslationWindow';
import './index.css';
import { normalizeSelectableLanguage } from './languages';
import { normalizeLanguage, setupI18n } from './i18n/config';
import { TranslationPayload, AppSettings, TranslateResponse, TranslateWindowState } from './types';

type TtsStatus = 'idle' | 'generating' | 'playing';
type TtsTarget = 'source' | 'translation';
type TtsSnapshot = {
  status: TtsStatus;
  target: TtsTarget | null;
  engine: string;
};

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

function resolveSourceLanguage(payload: TranslationPayload): string {
  return payload.detectedSourceLanguage?.toLowerCase() === 'auto'
    ? (payload.ocrDetectedSourceLanguage ?? payload.targetLanguage)
    : (payload.detectedSourceLanguage ?? payload.ocrDetectedSourceLanguage ?? payload.targetLanguage);
}

function getPreferredSourceLanguage(payload: TranslationPayload): string {
  return normalizeSelectableLanguage(resolveSourceLanguage(payload), 'auto');
}

function getPreferredTargetLanguage(payload: TranslationPayload): string {
  return normalizeSelectableLanguage(payload.targetLanguage, 'zh');
}

function TranslateWindowApp() {
  const [payload, setPayload] = useState<TranslationPayload>(placeholderPayload);
  const [isLoading, setIsLoading] = useState(false);
  const [isTranslating, setIsTranslating] = useState(false);
  const [ttsSnapshot, setTtsSnapshot] = useState<TtsSnapshot>({ status: 'idle', target: null, engine: 'edge' });
  const [selectedSourceLanguage, setSelectedSourceLanguage] = useState('auto');
  const [selectedTargetLanguage, setSelectedTargetLanguage] = useState(placeholderPayload.targetLanguage);
  const lastSnapshotRef = useRef('');

  const syncFromSnapshot = async () => {
    try {
      const [snapshot, nextTtsSnapshot] = await Promise.all([
        invoke<TranslateWindowState>('get_translate_window_state'),
        invoke<TtsSnapshot>('get_window_tts_snapshot'),
      ]);
      const nextKey = JSON.stringify(snapshot);

      if (nextKey === lastSnapshotRef.current) {
        setTtsSnapshot(nextTtsSnapshot);
        return;
      }

      lastSnapshotRef.current = nextKey;
      const nextPayload = buildPayloadFromSnapshot(snapshot);
      setPayload(nextPayload);
      setIsLoading(snapshot.is_loading);
      setIsTranslating(snapshot.is_translating);
      setTtsSnapshot(nextTtsSnapshot);
      setSelectedSourceLanguage(getPreferredSourceLanguage(nextPayload));
      setSelectedTargetLanguage(getPreferredTargetLanguage(nextPayload));
    } catch (error) {
      console.error('Failed to read translate window state', error);
    }
  };

  const handleTranslate = async (sourceLanguage: string, targetLanguage: string) => {
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
          source_language: sourceLanguage,
          target_language: targetLanguage,
        },
      });

      setPayload((prev) => ({
        ...prev,
        translatedText: response.translated_text,
        provider: response.provider,
        detectedSourceLanguage: response.detected_source_language ?? prev.detectedSourceLanguage,
        targetLanguage,
      }));
    } catch (error) {
      console.error('[TRANSLATE] manual translate failed', error);
    } finally {
      setIsTranslating(false);
    }
  };

  const handleSwap = async () => {
    try {
      await invoke('stop_window_tts');
      setTtsSnapshot((prev) => ({ ...prev, status: 'idle', target: null }));

      const nextSnapshot = await invoke<TranslateWindowState>('swap_translate_window_content');
      lastSnapshotRef.current = JSON.stringify(nextSnapshot);
      const nextPayload = buildPayloadFromSnapshot(nextSnapshot);
      setPayload(nextPayload);
      setIsLoading(nextSnapshot.is_loading);
      setIsTranslating(nextSnapshot.is_translating);
      setSelectedSourceLanguage(getPreferredSourceLanguage(nextPayload));
      setSelectedTargetLanguage(getPreferredTargetLanguage(nextPayload));
    } catch (error) {
      console.error('Failed to swap translation window content', error);
    }
  };

  const handleSpeakToggle = async (target: TtsTarget) => {
    const isSource = target === 'source';
    const text = isSource ? payload.sourceText : payload.translatedText;
    const language = isSource
      ? (selectedSourceLanguage === 'auto' ? (payload.detectedSourceLanguage || payload.ocrDetectedSourceLanguage || null) : selectedSourceLanguage)
      : (selectedTargetLanguage || payload.detectedSourceLanguage || null);

    if (!text.trim()) {
      return;
    }

    try {
      const isCurrentTarget = ttsSnapshot.target === target;
      const isBusy = ttsSnapshot.status !== 'idle';
      if (isCurrentTarget && isBusy) {
        await invoke('stop_window_tts');
        setTtsSnapshot((prev) => ({ ...prev, status: 'idle', target: null }));
        return;
      }

      setTtsSnapshot((prev) => ({ ...prev, status: 'generating', target }));
      await invoke('speak_window_text', {
        text,
        language,
        target,
      });
    } catch (error) {
      setTtsSnapshot((prev) => ({ ...prev, status: 'idle', target: null }));
      console.error('Failed to toggle window tts', error);
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

  useEffect(() => {
    if (!payload.sourceText && !payload.translatedText) {
      setTtsSnapshot((prev) => ({ ...prev, status: 'idle', target: null }));
      return;
    }

    void invoke('stop_window_tts')
      .then(() => {
        setTtsSnapshot((prev) => ({ ...prev, status: 'idle', target: null }));
      })
      .catch((error) => {
        console.error('Failed to stop window tts after text change', error);
      });
  }, [payload.sourceText, payload.translatedText]);

  useEffect(() => {
    return () => {
      void invoke('stop_window_tts').catch(() => {});
    };
  }, []);

  const sourceSpeechLoading = ttsSnapshot.target === 'source' && ttsSnapshot.status === 'generating';
  const translationSpeechLoading = ttsSnapshot.target === 'translation' && ttsSnapshot.status === 'generating';
  const sourceSpeaking = ttsSnapshot.target === 'source' && ttsSnapshot.status === 'playing';
  const translationSpeaking = ttsSnapshot.target === 'translation' && ttsSnapshot.status === 'playing';

  return (
    <TranslationWindow
      standalone
      payload={payload}
      isLoading={isLoading}
      isTranslating={isTranslating}
      sourceLanguage={selectedSourceLanguage}
      targetLanguage={selectedTargetLanguage}
      onSourceLanguageChange={setSelectedSourceLanguage}
      onTargetLanguageChange={setSelectedTargetLanguage}
      isSourceSpeaking={sourceSpeaking}
      isSourceSpeechLoading={sourceSpeechLoading}
      isTranslationSpeaking={translationSpeaking}
      isTranslationSpeechLoading={translationSpeechLoading}
      onTranslate={(sourceLanguage, targetLanguage) => {
        void handleTranslate(sourceLanguage, targetLanguage);
      }}
      onSwap={() => {
        void handleSwap();
      }}
      onSourceSpeakToggle={() => {
        void handleSpeakToggle('source');
      }}
      onTranslationSpeakToggle={() => {
        void handleSpeakToggle('translation');
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
