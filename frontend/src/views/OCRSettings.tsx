import { useState } from 'react';
import { ArrowRight, Keyboard } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';

export default function OCRSettings() {
  const { t } = useTranslation();
  const [enableShortcut, setEnableShortcut] = useState(true);
  const [autoTranslate, setAutoTranslate] = useState(false);

  return (
    <div className="max-w-4xl mx-auto animate-in fade-in duration-500">
      <section className="space-y-8">
        <div className="flex items-center gap-3 pb-2">
          <span className="text-[11px] font-bold text-primary uppercase tracking-[0.2em]">OCR Configuration</span>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div className="bg-surface-container-lowest p-6 rounded-xl shadow-sm border border-outline-variant/20 hover:bg-surface-container/50 transition-colors group">
            <div className="flex justify-between items-start">
              <div className="space-y-1.5 pr-4">
                <div className="font-bold text-on-surface text-base">{t('ocr.enableShortcut')}</div>
                <p className="text-xs text-on-surface-variant leading-relaxed">
                  Quickly trigger text capture using <kbd className="px-1.5 py-0.5 bg-surface-container border border-outline-variant/30 rounded text-[10px] font-mono ml-1">Alt + S</kbd>
                </p>
              </div>
              <Toggle checked={enableShortcut} onChange={setEnableShortcut} />
            </div>
          </div>

          <div className="bg-surface-container-lowest p-6 rounded-xl shadow-sm border border-outline-variant/20 hover:bg-surface-container/50 transition-colors group">
            <div className="flex justify-between items-start">
              <div className="space-y-1.5 pr-4">
                <div className="font-bold text-on-surface text-base">{t('ocr.autoTranslate')}</div>
                <p className="text-xs text-on-surface-variant leading-relaxed">
                  Automatically translate detected text after OCR processing
                </p>
              </div>
              <Toggle checked={autoTranslate} onChange={setAutoTranslate} />
            </div>
          </div>
        </div>

        <div className="space-y-6 pt-4">
          <div className="max-w-md">
            <label className="text-sm font-bold text-on-surface block mb-2">OCR Engine</label>
            <div className="relative group">
              <select className="w-full appearance-none bg-surface-container-lowest border border-outline-variant/30 px-4 py-3.5 rounded-xl text-sm font-medium focus:ring-2 focus:ring-primary/30 focus:border-primary outline-none cursor-pointer shadow-sm transition-all text-on-surface">
                <option value="standard">Standard (Fast, optimized for digital text)</option>
                <option value="deep_learning">Deep Learning (Advanced neural networks)</option>
              </select>
              <div className="absolute inset-y-0 right-4 flex items-center pointer-events-none text-on-surface-variant">
                <svg width="12" height="8" viewBox="0 0 12 8" fill="none" xmlns="http://www.w3.org/2000/svg">
                  <path d="M1.41 0.589966L6 5.16997L10.59 0.589966L12 1.99997L6 7.99997L0 1.99997L1.41 0.589966Z" fill="currentColor"/>
                </svg>
              </div>
            </div>
            <p className="mt-2 text-[11px] text-on-surface-variant italic">Select the processing engine based on your document type.</p>
          </div>

          <div className="max-w-md">
            <label className="text-sm font-bold text-on-surface block mb-2">Target Language</label>
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
            <p className="mt-2 text-[11px] text-on-surface-variant italic">Language used for the 'Auto-Translate' feature and OCR dictionary.</p>
          </div>
        </div>
      </section>

      <footer className="flex items-center justify-end pt-10 mt-10 border-t border-outline-variant/20">
        <div className="flex gap-4">
          <button className="px-6 py-2.5 text-sm font-bold text-on-surface-variant hover:bg-surface-container rounded-xl transition-colors">
            {t('ocr.discard')}
          </button>
          <button className="px-8 py-2.5 text-sm font-bold bg-primary text-white rounded-xl shadow-lg hover:scale-[1.02] active:scale-95 transition-all">
            {t('ocr.saveChanges')}
          </button>
        </div>
      </footer>
    </div>
  );
}
