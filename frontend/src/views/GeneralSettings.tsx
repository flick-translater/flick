import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Power, Database, FolderOpen, Keyboard, Edit2, Globe, LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';
import { AppSettings } from '../types';

type ShortcutField = keyof Pick<AppSettings, 'capture_shortcut' | 'translate_shortcut'>;

const shortcutCommandMap: Record<ShortcutField, 'update_capture_shortcut' | 'update_translate_shortcut'> = {
  capture_shortcut: 'update_capture_shortcut',
  translate_shortcut: 'update_translate_shortcut',
};

export default function GeneralSettings() {
  const { t, i18n } = useTranslation();
  const [startup, setStartup] = useState(true);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [recordingField, setRecordingField] = useState<ShortcutField | null>(null);
  const [savingField, setSavingField] = useState<ShortcutField | null>(null);
  const [shortcutError, setShortcutError] = useState('');
  const isMac = useMemo(
    () => /Mac|iPhone|iPad/i.test(navigator.platform),
    [],
  );
  const isWindows = useMemo(
    () => /Win/i.test(navigator.platform),
    [],
  );

  useEffect(() => {
    void invoke<AppSettings>('get_app_settings')
      .then(setSettings)
      .catch((error: unknown) => {
        setShortcutError(String(error));
      });
  }, []);

  useEffect(() => {
    if (!recordingField) {
      return;
    }

    void invoke('set_shortcut_recording', { recording: true }).catch((error: unknown) => {
      setShortcutError(String(error));
    });

    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      setShortcutError('');

      if (event.key === 'Escape') {
        void invoke('set_shortcut_recording', { recording: false });
        setRecordingField(null);
        return;
      }

      const shortcut = eventToShortcut(event);
      if (!shortcut) {
        setShortcutError(t('general.shortcutModifierHint'));
        return;
      }

      setSavingField(recordingField);
      void invoke<AppSettings>(shortcutCommandMap[recordingField], { shortcut })
        .then((updated) => {
          setSettings(updated);
          setRecordingField(null);
        })
        .catch((error: unknown) => {
          setShortcutError(String(error));
        })
        .finally(() => {
          void invoke('set_shortcut_recording', { recording: false });
          setSavingField(null);
        });
    };

    window.addEventListener('keydown', handleKeyDown, true);

    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
      void invoke('set_shortcut_recording', { recording: false });
    };
  }, [recordingField, t]);

  return (
    <div className="mx-auto max-w-5xl animate-in fade-in duration-500">
      <div className="grid grid-cols-1 gap-4 sm:gap-5 lg:grid-cols-2 lg:gap-6">
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
                    <path d="M1.41 0.589966L6 5.16997L10.59 0.589966L12 1.99997L6 7.99997L0 1.99997L1.41 0.589966Z" fill="currentColor" />
                  </svg>
                </div>
              </div>
            </div>
          </div>
        </section>

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
            <ShortcutCard
              label={t('general.action')}
              actionLabel={t('general.captureScreenshot')}
              shortcut={settings?.capture_shortcut}
              isRecording={recordingField === 'capture_shortcut'}
              isSaving={savingField === 'capture_shortcut'}
              onEdit={() => {
                setShortcutError('');
                setRecordingField('capture_shortcut');
              }}
              formatShortcut={(shortcut) => formatShortcut(shortcut, isMac, isWindows)}
              recordingText={t('general.recordingShortcut')}
            />
            <ShortcutCard
              label={t('general.action')}
              actionLabel={t('general.screenshotTranslate')}
              shortcut={settings?.translate_shortcut}
              isRecording={recordingField === 'translate_shortcut'}
              isSaving={savingField === 'translate_shortcut'}
              onEdit={() => {
                setShortcutError('');
                setRecordingField('translate_shortcut');
              }}
              formatShortcut={(shortcut) => formatShortcut(shortcut, isMac, isWindows)}
              recordingText={t('general.recordingShortcut')}
            />
          </div>
          <div className="mt-4 min-h-5 text-sm">
            {recordingField && (
              <p className="text-primary">
                {t('general.pressShortcut')}
              </p>
            )}
            {shortcutError && (
              <p className="text-error">{shortcutError}</p>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}

type ShortcutCardProps = {
  label: string;
  actionLabel: string;
  shortcut?: string;
  isRecording: boolean;
  isSaving: boolean;
  onEdit: () => void;
  formatShortcut: (shortcut?: string) => string[];
  recordingText: string;
};

function ShortcutCard({
  label,
  actionLabel,
  shortcut,
  isRecording,
  isSaving,
  onEdit,
  formatShortcut,
  recordingText,
}: ShortcutCardProps) {
  const tokens = formatShortcut(shortcut);

  return (
    <div className="flex flex-col gap-4 rounded-xl bg-surface-container-low p-5 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex flex-col">
        <span className="text-xs font-bold uppercase tracking-widest text-on-surface-variant opacity-70 mb-1">{label}</span>
        <span className="text-sm font-bold text-on-surface">{actionLabel}</span>
      </div>
      <div className="flex flex-wrap items-center gap-4">
        <div className="flex min-h-9 flex-wrap gap-1 items-center">
          {isRecording ? (
            <span className="text-sm font-semibold text-primary">{recordingText}</span>
          ) : (
            tokens.map((token, index) => (
              <div key={`${token}-${index}`} className="flex items-center gap-1">
                {index > 0 && <span className="text-on-surface-variant text-xs font-bold">+</span>}
                <kbd className="px-2.5 py-1.5 bg-white text-primary border border-outline-variant/30 rounded-lg text-xs font-bold shadow-sm">
                  {token}
                </kbd>
              </div>
            ))
          )}
        </div>
        <button
          onClick={onEdit}
          disabled={isSaving}
          className="p-2 text-primary hover:bg-white rounded-lg transition-colors shadow-sm disabled:opacity-50"
        >
          {isSaving ? <LoaderCircle size={16} className="animate-spin" /> : <Edit2 size={16} />}
        </button>
      </div>
    </div>
  );
}

function eventToShortcut(event: KeyboardEvent) {
  const modifiers = [
    ...(event.ctrlKey ? ['Ctrl'] : []),
    ...(event.metaKey ? ['Super'] : []),
    ...(event.altKey ? ['Alt'] : []),
    ...(event.shiftKey ? ['Shift'] : []),
  ];
  const key = normalizeShortcutKey(event);

  if (!key || modifiers.length === 0) {
    return null;
  }

  return [...modifiers, key].join('+');
}

function normalizeShortcutKey(event: KeyboardEvent) {
  const { key, code } = event;

  if (['Meta', 'Control', 'Alt', 'Shift'].includes(key)) {
    return null;
  }

  if (code.startsWith('Key')) {
    return code.slice(3).toUpperCase();
  }

  if (code.startsWith('Digit')) {
    return code.slice(5);
  }

  if (code.startsWith('Numpad')) {
    return code;
  }

  if (/^F\d{1,2}$/.test(code)) {
    return code;
  }

  if (key.length === 1) {
    return key.toUpperCase();
  }

  const aliases: Record<string, string> = {
    Escape: 'Esc',
    ' ': 'Space',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
  };

  return aliases[key] ?? key;
}

function formatShortcut(shortcut: string | undefined, isMac: boolean, isWindows: boolean) {
  if (!shortcut) {
    return ['...'];
  }

  return shortcut.split('+').map((part) => {
    if (part === 'Ctrl' || part === 'Control') {
      return 'Ctrl';
    }

    if (part === 'Super' || part === 'Meta' || part === 'Cmd' || part === 'Command') {
      if (isMac) {
        return 'Cmd';
      }

      if (isWindows) {
        return 'Win';
      }

      return 'Super';
    }

    if (part === 'CommandOrControl') {
      return isMac ? 'Cmd' : 'Ctrl';
    }

    if (part === 'Alt') {
      return isMac ? 'Option' : 'Alt';
    }

    return part;
  });
}
