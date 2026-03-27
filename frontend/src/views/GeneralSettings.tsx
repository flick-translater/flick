import { useState } from 'react';
import { Power, Database, FolderOpen, Keyboard, Edit2, Globe } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';

export default function GeneralSettings() {
  const { t, i18n } = useTranslation();
  const [startup, setStartup] = useState(true);

  return (
    <div className="mx-auto max-w-5xl animate-in fade-in duration-500">
      <div className="grid grid-cols-1 gap-4 sm:gap-5 lg:grid-cols-2 lg:gap-6">
        {/* System Language */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/20 p-5 shadow-sm transition-shadow duration-300 hover:shadow-md lg:col-span-2 sm:p-6 lg:p-8">
          <div className="mb-6 flex items-start justify-between gap-4 sm:mb-8">
            <div>
              <h2 className="text-lg font-bold text-primary mb-1 font-headline">{t('general.systemLanguage')}</h2>
              <p className="text-sm text-on-surface-variant">{t('general.systemLanguageDesc')}</p>
            </div>
            <div className="p-3 bg-primary/5 rounded-lg text-primary">
              <Globe size={24} />
            </div>
          </div>
          <div className="flex flex-col sm:flex-row gap-4">
            <div className="flex-1 relative group">
              <label className="block text-[11px] font-bold uppercase tracking-widest text-on-surface-variant mb-2 ml-1">{t('general.interfaceLanguage')}</label>
              <div className="relative">
                <select 
                  value={i18n.language}
                  onChange={(e) => i18n.changeLanguage(e.target.value)}
                  className="w-full bg-surface-container border border-outline-variant/30 rounded-xl py-3.5 px-4 text-sm appearance-none focus:ring-2 focus:ring-primary outline-none cursor-pointer text-on-surface font-medium transition-all"
                >
                  <option value="en">English</option>
                  <option value="zh">简体中文</option>
                  <option value="ja">日本語</option>
                </select>
                <div className="absolute inset-y-0 right-4 flex items-center pointer-events-none text-on-surface-variant">
                  <svg width="12" height="8" viewBox="0 0 12 8" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <path d="M1.41 0.589966L6 5.16997L10.59 0.589966L12 1.99997L6 7.99997L0 1.99997L1.41 0.589966Z" fill="currentColor"/>
                  </svg>
                </div>
              </div>
            </div>
          </div>
        </section>

        {/* App Startup */}
        <section className="flex flex-col justify-between rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-5 shadow-sm transition-shadow duration-300 hover:shadow-md sm:p-6 lg:p-8">
          <div className="flex items-start justify-between">
            <div>
              <h2 className="text-lg font-bold text-primary mb-1 font-headline">{t('general.appStartup')}</h2>
              <p className="text-sm text-on-surface-variant leading-relaxed">{t('general.appStartupDesc')}</p>
            </div>
            <div className="p-3 bg-primary/5 rounded-lg text-primary">
              <Power size={24} />
            </div>
          </div>
          <div className="mt-8 flex items-center justify-between">
            <span className="text-sm font-semibold text-on-surface">{t('general.launchAtStartup')}</span>
            <Toggle checked={startup} onChange={setStartup} />
          </div>
        </section>

        {/* Data Retention */}
        <section className="flex flex-col justify-between rounded-xl bg-surface-container p-5 shadow-sm transition-shadow duration-300 hover:shadow-md sm:p-6 lg:p-8">
          <div className="flex items-start justify-between">
            <div>
              <h2 className="text-lg font-bold text-primary mb-1 font-headline">{t('general.dataRetention')}</h2>
              <p className="text-sm text-on-surface-variant leading-relaxed">{t('general.dataRetentionDesc')}</p>
            </div>
            <div className="p-3 bg-primary/5 rounded-lg text-primary">
              <Database size={24} />
            </div>
          </div>
          <div className="mt-8 space-y-4">
            <div className="flex justify-between items-end">
              <label htmlFor="retention" className="text-sm font-semibold text-on-surface">{t('general.maxScreenshots')}</label>
              <span className="text-xl font-black text-primary font-headline">500 <span className="text-xs font-normal text-on-surface-variant">{t('general.items')}</span></span>
            </div>
            <input 
              type="range" 
              id="retention" 
              min="50" 
              max="1000" 
              defaultValue="500" 
              className="w-full h-1.5 bg-surface-container-highest rounded-lg appearance-none cursor-pointer accent-primary-container"
            />
          </div>
        </section>

        {/* Storage Path */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/20 p-5 shadow-sm transition-shadow duration-300 hover:shadow-md lg:col-span-2 sm:p-6 lg:p-8">
          <div className="mb-6 flex items-start justify-between gap-4 sm:mb-8">
            <div>
              <h2 className="text-lg font-bold text-primary mb-1 font-headline">{t('general.storagePath')}</h2>
              <p className="text-sm text-on-surface-variant">{t('general.storagePathDesc')}</p>
            </div>
            <div className="p-3 bg-primary/5 rounded-lg text-primary">
              <FolderOpen size={24} />
            </div>
          </div>
          <div className="flex flex-col sm:flex-row gap-4">
            <div className="flex-1 relative group">
              <label className="block text-[11px] font-bold uppercase tracking-widest text-on-surface-variant mb-2 ml-1">{t('general.screenshotSavePath')}</label>
              <div className="flex min-w-0 items-center rounded-xl border border-transparent bg-surface-container px-4 py-3.5 transition-all group-focus-within:border-primary-container/30">
                <FolderOpen className="text-on-surface-variant mr-3" size={18} />
                <span className="truncate text-sm font-medium text-on-surface opacity-80">/Users/User/Pictures/Flick</span>
              </div>
            </div>
            <div className="flex items-end">
              <button className="w-full sm:w-auto h-[52px] px-6 bg-surface-container-highest text-primary font-bold text-sm rounded-xl hover:bg-primary hover:text-white transition-all duration-200">
                {t('general.changePath')}
              </button>
            </div>
          </div>
        </section>

        {/* Global Hotkeys */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/20 p-5 shadow-sm transition-shadow duration-300 hover:shadow-md lg:col-span-2 sm:p-6 lg:p-8">
          <div className="mb-6 flex items-start justify-between gap-4 sm:mb-8">
            <div>
              <h2 className="text-lg font-bold text-primary mb-1 font-headline">{t('general.globalHotkeys')}</h2>
              <p className="text-sm text-on-surface-variant">{t('general.globalHotkeysDesc')}</p>
            </div>
            <div className="p-3 bg-primary/5 rounded-lg text-primary">
              <Keyboard size={24} />
            </div>
          </div>
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <div className="flex flex-col gap-4 rounded-xl bg-surface-container-low p-5 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex flex-col">
                <span className="text-xs font-bold uppercase tracking-widest text-on-surface-variant opacity-70 mb-1">{t('general.action')}</span>
                <span className="text-sm font-bold text-on-surface">{t('general.captureScreenshot')}</span>
              </div>
              <div className="flex flex-wrap items-center gap-4">
                <div className="flex gap-1 items-center">
                  <kbd className="px-2.5 py-1.5 bg-white text-primary border border-outline-variant/30 rounded-lg text-xs font-bold shadow-sm">Alt</kbd>
                  <span className="text-on-surface-variant text-xs font-bold">+</span>
                  <kbd className="px-2.5 py-1.5 bg-white text-primary border border-outline-variant/30 rounded-lg text-xs font-bold shadow-sm">A</kbd>
                </div>
                <button className="p-2 text-primary hover:bg-white rounded-lg transition-colors shadow-sm">
                  <Edit2 size={16} />
                </button>
              </div>
            </div>
            <div className="flex flex-col gap-4 rounded-xl bg-surface-container-low p-5 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex flex-col">
                <span className="text-xs font-bold uppercase tracking-widest text-on-surface-variant opacity-70 mb-1">{t('general.action')}</span>
                <span className="text-sm font-bold text-on-surface">{t('general.screenshotTranslate')}</span>
              </div>
              <div className="flex flex-wrap items-center gap-4">
                <div className="flex gap-1 items-center">
                  <kbd className="px-2.5 py-1.5 bg-white text-primary border border-outline-variant/30 rounded-lg text-xs font-bold shadow-sm">Alt</kbd>
                  <span className="text-on-surface-variant text-xs font-bold">+</span>
                  <kbd className="px-2.5 py-1.5 bg-white text-primary border border-outline-variant/30 rounded-lg text-xs font-bold shadow-sm">T</kbd>
                </div>
                <button className="p-2 text-primary hover:bg-white rounded-lg transition-colors shadow-sm">
                  <Edit2 size={16} />
                </button>
              </div>
            </div>
          </div>
        </section>
      </div>

      <footer className="mt-8 flex flex-col-reverse gap-3 sm:mt-10 sm:flex-row sm:items-center sm:justify-end sm:gap-4 lg:mt-12">
        <button className="w-full px-6 py-2.5 text-sm font-bold text-on-surface-variant transition-colors hover:text-primary sm:w-auto">{t('general.discardChanges')}</button>
        <button className="w-full rounded-xl bg-primary-container px-8 py-3 text-sm font-bold text-white shadow-lg transition-all hover:opacity-90 active:scale-95 sm:w-auto">{t('general.savePreferences')}</button>
      </footer>
    </div>
  );
}
