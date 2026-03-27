import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './locales/en';
import zh from './locales/zh';
import ja from './locales/ja';

export function normalizeLanguage(language?: string | null) {
  const normalized = language?.split(/[-_]/)[0]?.toLowerCase();
  if (normalized === 'zh' || normalized === 'ja') {
    return normalized;
  }
  return 'en';
}

export async function setupI18n(initialLanguage?: string) {
  if (!i18n.isInitialized) {
    await i18n.use(initReactI18next).init({
      resources: {
        en,
        zh,
        ja,
      },
      lng: normalizeLanguage(initialLanguage),
      fallbackLng: 'en',
      interpolation: {
        escapeValue: false,
      },
    });
    return i18n;
  }

  await i18n.changeLanguage(normalizeLanguage(initialLanguage));
  return i18n;
}

export default i18n;
