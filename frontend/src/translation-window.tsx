import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import TranslationWidget from './components/TranslationWidget';
import './index.css';
import './i18n/config';
import { TranslationPayload } from './types';

const placeholderPayload: TranslationPayload = {
  imagePath: '',
  sourceText: '等待翻译结果',
  translatedText: '使用截图翻译快捷键后，结果会显示在这里。',
  provider: '',
  detectedSourceLanguage: null,
  targetLanguage: 'zh',
};

function WidgetApp() {
  const [payload, setPayload] = useState<TranslationPayload>(placeholderPayload);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    void listen<TranslationPayload>('translation-ready', (event) => {
      setPayload(event.payload);
    }).then((dispose) => {
      unlisten = dispose;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  return (
    <TranslationWidget
      standalone
      payload={payload}
      onClose={() => {
        void getCurrentWindow().close();
      }}
    />
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <WidgetApp />
  </StrictMode>,
);
