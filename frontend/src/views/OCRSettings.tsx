import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';

export default function OCRSettings() {
  const { t } = useTranslation();
  const [enableShortcut, setEnableShortcut] = useState(true);
  const [autoTranslate, setAutoTranslate] = useState(false);

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
              <Toggle checked={enableShortcut} onChange={setEnableShortcut} />
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
              <Toggle checked={autoTranslate} onChange={setAutoTranslate} />
            </div>
          </div>
        </div>

        <div className="space-y-6 pt-4">
          <div className="max-w-md">
            <label className="text-sm font-bold text-on-surface block mb-2">{t('ocr.ocrEngine')}</label>
            <div className="relative group">
              <select className="w-full appearance-none bg-surface-container-lowest border border-outline-variant/30 px-4 py-3.5 rounded-xl text-sm font-medium focus:ring-2 focus:ring-primary/30 focus:border-primary outline-none cursor-pointer shadow-sm transition-all text-on-surface">
                <option value="standard">{t('ocr.ocrEngineStandard')}</option>
                <option value="deep_learning">{t('ocr.ocrEngineDeepLearning')}</option>
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
              <select className="w-full appearance-none bg-surface-container-lowest border border-outline-variant/30 px-4 py-3.5 rounded-xl text-sm font-medium focus:ring-2 focus:ring-primary/30 focus:border-primary outline-none cursor-pointer shadow-sm transition-all text-on-surface">
                <option>English (United States)</option>
                <option>Spanish (Mexico)</option>
                <option>French (France)</option>
                <option>German (Germany)</option>
                <option>Japanese (Japan)</option>
                <option>Chinese (Simplified)</option>
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