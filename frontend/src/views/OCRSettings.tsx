import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';
import type { AppSettings, OcrEngineInfo } from '../types';

const languageMap: Record<string, string> = {
  en: 'en', 'en-US': 'en', 'en-GB': 'en', 'en-AU': 'en', 'en-CA': 'en',
  zh: 'zh', 'zh-CN': 'zh', 'zh-HK': 'zh-tw', 'zh-TW': 'zh-tw',
  ja: 'ja', 'ja-JP': 'ja',
  ko: 'ko', 'ko-KR': 'ko',
  es: 'es', 'es-ES': 'es', 'es-MX': 'es', 'es-AR': 'es',
  fr: 'fr', 'fr-FR': 'fr', 'fr-CA': 'fr',
  de: 'de', 'de-DE': 'de', 'de-AT': 'de',
  it: 'it', 'it-IT': 'it',
  pt: 'pt', 'pt-PT': 'pt', 'pt-BR': 'pt',
  ru: 'ru', 'ru-RU': 'ru',
  ar: 'ar', 'ar-SA': 'ar', 'ar-AE': 'ar',
  th: 'th', 'th-TH': 'th',
  vi: 'vi', 'vi-VN': 'vi',
  nl: 'nl', 'nl-NL': 'nl', 'nl-BE': 'nl',
  pl: 'pl', 'pl-PL': 'pl',
  tr: 'tr', 'tr-TR': 'tr',
  id: 'id', 'id-ID': 'id',
  hi: 'hi', 'hi-IN': 'hi',
};

function getDefaultLanguage(): string {
  const browserLang = navigator.language;
  return languageMap[browserLang] || 'en';
}

export default function OCRSettings() {
  const { t } = useTranslation();
  const [enableShortcut, setEnableShortcut] = useState(true);
  const [autoTranslate, setAutoTranslate] = useState(true);
  const [targetLanguage, setTargetLanguage] = useState(getDefaultLanguage);
  const [ocrProvider, setOcrProvider] = useState('');
  const [availableEngines, setAvailableEngines] = useState<OcrEngineInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const targetLanguages = useMemo(() => [
    { value: 'en', label: t('ocr.languages.english') },
    { value: 'zh', label: t('ocr.languages.chineseSimplified') },
    { value: 'zh-tw', label: t('ocr.languages.chineseTraditional') },
    { value: 'ja', label: t('ocr.languages.japanese') },
    { value: 'ko', label: t('ocr.languages.korean') },
    { value: 'es', label: t('ocr.languages.spanish') },
    { value: 'fr', label: t('ocr.languages.french') },
    { value: 'de', label: t('ocr.languages.german') },
    { value: 'it', label: t('ocr.languages.italian') },
    { value: 'pt', label: t('ocr.languages.portuguese') },
    { value: 'ru', label: t('ocr.languages.russian') },
    { value: 'ar', label: t('ocr.languages.arabic') },
    { value: 'th', label: t('ocr.languages.thai') },
    { value: 'vi', label: t('ocr.languages.vietnamese') },
    { value: 'nl', label: t('ocr.languages.dutch') },
    { value: 'pl', label: t('ocr.languages.polish') },
    { value: 'tr', label: t('ocr.languages.turkish') },
    { value: 'id', label: t('ocr.languages.indonesian') },
    { value: 'hi', label: t('ocr.languages.hindi') },
  ], [t]);

  const ocrEngineLabel = (engineId: string) => {
    switch (engineId) {
      case 'vision':
        return t('ocr.engines.macosVision', { defaultValue: 'macOS Vision (Built-in)' });
      case 'windows':
        return t('ocr.engines.windowsBuiltin', { defaultValue: 'Windows OCR (Built-in)' });
      case 'onnx':
        return t('ocr.engines.onnxRuntime', { defaultValue: 'ONNX Runtime (Bundled Models)' });
      default:
        return engineId;
    }
  };

  useEffect(() => {
    void Promise.all([
      invoke<AppSettings>('get_app_settings'),
      invoke<OcrEngineInfo[]>('get_available_ocr_engines'),
    ])
      .then(([settings, engines]) => {
        setAvailableEngines(engines);
        setEnableShortcut(settings.ocr_shortcut_enabled);
        setAutoTranslate(settings.ocr_auto_translate);
        setTargetLanguage(settings.ocr_target_language || getDefaultLanguage());
        setOcrProvider(settings.ocr_provider);
      })
      .finally(() => {
        setIsLoading(false);
      });
  }, []);

  const handleShortcutToggle = (checked: boolean) => {
    setEnableShortcut(checked);
    void invoke<AppSettings>('update_ocr_shortcut_enabled', { enabled: checked })
      .then((settings) => {
        setEnableShortcut(settings.ocr_shortcut_enabled);
      })
      .catch(() => {
        setEnableShortcut(!checked);
      });
  };

  const handleAutoTranslateToggle = (checked: boolean) => {
    setAutoTranslate(checked);
    void invoke<AppSettings>('update_ocr_auto_translate', { enabled: checked })
      .then((settings) => {
        setAutoTranslate(settings.ocr_auto_translate);
      })
      .catch(() => {
        setAutoTranslate(!checked);
      });
  };

  const handleTargetLanguageChange = (language: string) => {
    setTargetLanguage(language);
    void invoke<AppSettings>('update_ocr_target_language', { language })
      .then((settings) => {
        setTargetLanguage(settings.ocr_target_language);
      })
      .catch(() => {
        setTargetLanguage(getDefaultLanguage());
      });
  };

  const handleOcrProviderChange = (provider: string) => {
    const previousProvider = ocrProvider;
    setOcrProvider(provider);
    void invoke<AppSettings>('update_ocr_provider', { provider })
      .then((settings) => {
        setOcrProvider(settings.ocr_provider);
      })
      .catch(() => {
        setOcrProvider(previousProvider);
      });
  };

  if (isLoading) {
    return (
      <div className="mx-auto max-w-4xl animate-in fade-in duration-500">
        <section className="rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-5 text-sm text-on-surface-variant shadow-sm sm:p-6">
          Loading...
        </section>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-4xl animate-in fade-in duration-500">
      <section className="space-y-8">
        <div className="grid grid-cols-1 gap-4 sm:gap-5 lg:grid-cols-2 lg:gap-6">
          <div className="group rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-5 shadow-sm transition-colors hover:bg-surface-container/50 sm:p-6">
            <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
              <div className="space-y-1.5 pr-4">
                <div className="font-bold text-on-surface text-base">{t('ocr.enableShortcut')}</div>
                <p className="text-xs text-on-surface-variant leading-relaxed">
                  {t('ocr.triggerOcrDesc')}
                </p>
              </div>
              <Toggle checked={enableShortcut} onChange={handleShortcutToggle} />
            </div>
          </div>

          <div className="group rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-5 shadow-sm transition-colors hover:bg-surface-container/50 sm:p-6">
            <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
              <div className="space-y-1.5 pr-4">
                <div className="font-bold text-on-surface text-base">{t('ocr.autoTranslate')}</div>
                <p className="text-xs text-on-surface-variant leading-relaxed">
                  {t('ocr.autoTranslateDesc')}
                </p>
              </div>
              <Toggle checked={autoTranslate} onChange={handleAutoTranslateToggle} />
            </div>
          </div>
        </div>

        <div className="space-y-6 pt-4">
          <div className="max-w-md">
            <label className="text-sm font-bold text-on-surface block mb-2">{t('ocr.ocrEngine')}</label>
            <div className="relative group">
              <select
                value={ocrProvider}
                disabled={availableEngines.length === 0}
                onChange={(e) => handleOcrProviderChange(e.target.value)}
                className="w-full appearance-none bg-surface-container-lowest border border-outline-variant/30 px-4 py-3.5 rounded-xl text-sm font-medium focus:ring-2 focus:ring-primary/30 focus:border-primary outline-none cursor-pointer shadow-sm transition-all text-on-surface disabled:cursor-not-allowed disabled:opacity-60"
              >
                {availableEngines.length > 0 ? availableEngines.map((engine) => (
                  <option key={engine.id} value={engine.id}>{ocrEngineLabel(engine.id)}</option>
                )) : (
                  <option value="">{t('ocr.noEngineAvailable')}</option>
                )}
              </select>
              <div className="absolute inset-y-0 right-4 flex items-center pointer-events-none text-on-surface-variant">
                <svg width="12" height="8" viewBox="0 0 12 8" fill="none" xmlns="http://www.w3.org/2000/svg">
                  <path d="M1.41 0.589966L6 5.16997L10.59 0.589966L12 1.99997L6 7.99997L0 1.99997L1.41 0.589966Z" fill="currentColor"/>
                </svg>
              </div>
            </div>
            <p className="mt-2 text-[11px] text-on-surface-variant italic">{t('ocr.ocrEngineHint')}</p>
          </div>

          <div className="max-w-md">
            <label className="text-sm font-bold text-on-surface block mb-2">{t('ocr.targetLanguage')}</label>
            <div className="relative group">
              <select value={targetLanguage} onChange={(e) => handleTargetLanguageChange(e.target.value)} className="w-full appearance-none bg-surface-container-lowest border border-outline-variant/30 px-4 py-3.5 rounded-xl text-sm font-medium focus:ring-2 focus:ring-primary/30 focus:border-primary outline-none cursor-pointer shadow-sm transition-all text-on-surface">
                {targetLanguages.map(lang => (
                  <option key={lang.value} value={lang.value}>{lang.label}</option>
                ))}
              </select>
              <div className="absolute inset-y-0 right-4 flex items-center pointer-events-none text-on-surface-variant">
                <svg width="12" height="8" viewBox="0 0 12 8" fill="none" xmlns="http://www.w3.org/2000/svg">
                  <path d="M1.41 0.589966L6 5.16997L10.59 0.589966L12 1.99997L6 7.99997L0 1.99997L1.41 0.589966Z" fill="currentColor"/>
                </svg>
              </div>
            </div>
            <p className="mt-2 text-[11px] text-on-surface-variant italic">{t('ocr.targetLanguageHint')}</p>
          </div>
        </div>
      </section>
    </div>
  );
}
