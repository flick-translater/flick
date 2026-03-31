import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import TranslationWidget from './components/TranslationWidget';
import './index.css';
import './i18n/config';
import { TranslationPayload } from './types';

window.addEventListener('contextmenu', (event) => {
  event.preventDefault();
});

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
    console.log('[WIDGET] WidgetApp mounted, setting up event listener');
    let unlisten: (() => void) | undefined;

    void listen<TranslationPayload>('translation-ready', (event) => {
      console.log('[WIDGET] translation-ready event received');
      console.log('[WIDGET] event payload:', event.payload);
      setPayload(event.payload);
    }).then((dispose) => {
      console.log('[WIDGET] event listener setup complete');
      unlisten = dispose;
    });

    return () => {
      console.log('[WIDGET] cleaning up event listener');
      unlisten?.();
    };
  }, []);

  console.log('[WIDGET] current payload state:', payload);

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
