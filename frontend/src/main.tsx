import {StrictMode} from 'react';
import {createRoot} from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import App from './App.tsx';
import './index.css';
import { normalizeLanguage, setupI18n } from './i18n/config';
import type { AppSettings } from './types';

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
      <App />
    </StrictMode>,
  );
}

void bootstrap();
