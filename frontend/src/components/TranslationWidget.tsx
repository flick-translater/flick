import { useEffect, useState, type MouseEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Pin, Minus, X, Copy, ArrowRightLeft, Volume2, ScanText, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TranslationPayload } from '../types';
import { useTypewriter } from '../hooks/useTypewriter';

interface TranslationWidgetProps {
  payload: TranslationPayload;
  isLoading?: boolean;
  isTranslating?: boolean;
  onClose: () => void;
  onTranslate?: () => void;
  standalone?: boolean;
}

function languageLabel(code: string | null | undefined, t: (key: string) => string) {
  switch (code?.toLowerCase()) {
    case 'zh':
    case 'zh-hans':
    case 'zh-hant':
      return t('widget.chinese');
    case 'ja':
      return t('widget.japanese');
    case 'ko':
      return t('widget.korean');
    case 'ar':
      return t('widget.arabic');
    case 'de':
      return t('widget.german');
    case 'fr':
      return t('widget.french');
    case 'it':
      return t('widget.italian');
    case 'nl':
      return t('widget.dutch');
    case 'ru':
      return t('widget.russian');
    case 'th':
      return t('widget.thai');
    case 'he':
      return t('widget.hebrew');
    case 'el':
      return t('widget.greek');
    case 'hi':
      return t('widget.hindi');
    case 'en':
      return t('widget.english');
    case 'auto':
      return t('widget.auto');
    default:
      return t('widget.unknownLanguage');
  }
}

export default function TranslationWidget({ payload, isLoading = false, isTranslating = false, onClose, onTranslate, standalone = false }: TranslationWidgetProps) {
  const { t } = useTranslation();
  const [isPinned, setIsPinned] = useState(false);
  const [sourceCopied, setSourceCopied] = useState(false);
  const [translationCopied, setTranslationCopied] = useState(false);
  const windowHandle = standalone ? getCurrentWindow() : null;
  const isTranslateDisabled = isLoading || isTranslating || !payload.sourceText.trim();
  const resolvedSourceLanguage = payload.detectedSourceLanguage?.toLowerCase() === 'auto'
    ? (payload.ocrDetectedSourceLanguage ?? 'auto')
    : (payload.detectedSourceLanguage ?? payload.ocrDetectedSourceLanguage);
  const sourceLanguageText = languageLabel(resolvedSourceLanguage, t);
  const targetLanguageText = languageLabel(payload.targetLanguage, t);

  const { displayedText: displayedSource, isTyping: isTypingSource } = useTypewriter(
    payload.sourceText,
    { enabled: standalone }
  );

  const { displayedText: displayedTranslation, isTyping: isTypingTranslation } = useTypewriter(
    payload.translatedText,
    { enabled: standalone }
  );

  useEffect(() => {
    if (!windowHandle) {
      return;
    }

    void invoke<boolean>('get_translation_widget_pinned').then(setIsPinned).catch((error) => {
      console.error('Failed to read always-on-top state', error);
    });
  }, [windowHandle]);

  const handleHeaderMouseDown = async (event: MouseEvent<HTMLElement>) => {
    if (!standalone || event.button !== 0) {
      return;
    }

    const target = event.target as HTMLElement | null;
    if (target?.closest('button, input, select, textarea, a, [role="button"]')) {
      return;
    }

    event.preventDefault();

    try {
      await invoke('begin_translation_widget_drag');
    } catch (error) {
      try {
        await windowHandle?.setFocus();
        await windowHandle?.startDragging();
      } catch (fallbackError) {
        console.error('Failed to start dragging widget window', fallbackError ?? error);
      }
    }
  };

  const handleTogglePinned = async () => {
    const next = !isPinned;

    try {
      await invoke('set_translation_widget_pinned', { pinned: next });
      setIsPinned(next);
    } catch (error) {
      console.error('Failed to toggle always-on-top', error);
    }
  };

  const handleMinimize = async () => {
    try {
      await invoke('minimize_translation_widget');
    } catch (error) {
      console.error('Failed to minimize widget window', error);
    }
  };

  const handleClose = async () => {
    try {
      if (standalone) {
        await invoke('close_translation_widget');
        return;
      }
      onClose();
    } catch (error) {
      console.error('Failed to close widget window', error);
    }
  };

  const handleCopySource = async () => {
    if (!payload.sourceText) {
      return;
    }

    try {
      await navigator.clipboard.writeText(payload.sourceText);
      setSourceCopied(true);
      window.setTimeout(() => {
        setSourceCopied(false);
      }, 1200);
    } catch (error) {
      console.error('Failed to copy source text', error);
    }
  };

  const handleCopyTranslation = async () => {
    if (!payload.translatedText) {
      return;
    }

    try {
      await navigator.clipboard.writeText(payload.translatedText);
      setTranslationCopied(true);
      window.setTimeout(() => {
        setTranslationCopied(false);
      }, 1200);
    } catch (error) {
      console.error('Failed to copy translated text', error);
    }
  };

  return (
    <div className={standalone
      ? 'flex h-screen flex-col overflow-hidden rounded-[18px] border border-outline-variant/30 bg-surface shadow-2xl'
      : 'fixed inset-x-3 bottom-3 top-24 z-50 flex max-h-[calc(100vh-7rem)] flex-col overflow-hidden rounded-xl border border-outline-variant/30 shadow-2xl duration-300 animate-in fade-in glass-panel sm:left-auto sm:right-4 sm:w-[min(480px,calc(100vw-2rem))] lg:right-8 lg:top-20 lg:h-[min(640px,calc(100vh-6rem))] lg:max-h-none'}
    >
      {/* Header */}
      <header
        className={`flex justify-between items-center px-4 py-3 bg-white/80 border-b border-outline-variant/20 shrink-0 ${standalone ? 'select-none' : ''}`}
        onMouseDown={(event) => {
          void handleHeaderMouseDown(event);
        }}
      >
        <div className="flex items-center gap-2">
          <span className="cursor-default font-headline text-2xl font-black leading-tight tracking-tight text-primary">Flick</span>
        </div>
        <div className="flex gap-1">
          <button
            onMouseDown={(event) => {
              event.stopPropagation();
            }}
            onClick={() => {
              void handleTogglePinned();
            }}
            className={`w-8 h-8 flex items-center justify-center rounded transition-colors ${isPinned ? 'bg-primary text-white' : 'text-on-surface-variant hover:bg-surface-container'}`}
          >
            <Pin size={16} className={isPinned ? 'fill-current' : ''} />
          </button>
          <button
            onMouseDown={(event) => {
              event.stopPropagation();
            }}
            onClick={() => {
              void handleMinimize();
            }}
            className="w-8 h-8 flex items-center justify-center rounded hover:bg-surface-container transition-colors text-on-surface-variant"
          >
            <Minus size={16} />
          </button>
          <button
            onMouseDown={(event) => {
              event.stopPropagation();
            }}
            onClick={() => {
              void handleClose();
            }}
            className="w-8 h-8 flex items-center justify-center rounded hover:bg-error/10 hover:text-error transition-colors text-on-surface-variant"
          >
            <X size={16} />
          </button>
        </div>
      </header>

      {/* Main Content */}
      <main className="flex flex-1 flex-col gap-4 overflow-hidden bg-surface/50 p-4 sm:p-5">
        {/* Source Text */}
        <section className="flex-1 flex flex-col min-h-0 bg-white border border-outline-variant/30 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-3 border-b border-outline-variant/20 pb-2">
            <span className="min-w-0 flex-1 text-[10px] uppercase font-bold tracking-[0.1em] text-outline">{t('widget.sourceText')}</span>
            <button
              onClick={() => {
                void handleCopySource();
              }}
              className={`ml-auto flex shrink-0 items-center justify-end text-on-surface-variant hover:text-primary transition-colors ${sourceCopied ? 'text-primary' : ''}`}
            >
              {sourceCopied ? (
                <span className="text-[11px] font-bold">{t('history.copied')}</span>
              ) : (
                <Copy size={16} />
              )}
            </button>
          </div>
          <div className="flex-1 overflow-y-auto custom-scrollbar pr-2">
            {isLoading && !payload.sourceText ? (
              <div className="flex items-center gap-2 text-on-surface-variant">
                <Loader2 size={16} className="animate-spin" />
                <span className="text-sm">{t('widget.recognizing', { defaultValue: '识别中...' })}</span>
              </div>
            ) : (
              <p className="font-body text-sm leading-relaxed text-on-surface">
                {standalone ? displayedSource : payload.sourceText}
                {isTypingSource && <span className="inline-block w-0.5 h-4 bg-primary ml-0.5 animate-pulse" />}
              </p>
            )}
          </div>
        </section>

        {/* Language Selector */}
        <div className="grid shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-2">
          <div className="relative min-w-0">
            <select className="w-full appearance-none py-2.5 px-3 rounded-lg border border-outline-variant/30 bg-white text-on-surface font-medium text-xs hover:border-primary/50 transition-all outline-none cursor-pointer shadow-sm">
              <option>{sourceLanguageText}</option>
            </select>
            <div className="absolute inset-y-0 right-3 flex items-center pointer-events-none text-on-surface-variant">
              <svg width="10" height="6" viewBox="0 0 10 6" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M1.175 0.150024L5 3.97502L8.825 0.150024L10 1.32502L5 6.32502L0 1.32502L1.175 0.150024Z" fill="currentColor"/>
              </svg>
            </div>
          </div>
          
          <button className="w-10 h-10 rounded-lg bg-primary text-white flex items-center justify-center shadow-md hover:bg-primary-container transition-all active:scale-95 shrink-0">
            <ArrowRightLeft size={18} />
          </button>
          
          <div className="relative min-w-0">
            <select className="w-full appearance-none py-2.5 px-3 rounded-lg border border-outline-variant/30 bg-white text-on-surface font-medium text-xs hover:border-primary/50 transition-all outline-none cursor-pointer shadow-sm">
              <option>{targetLanguageText}</option>
            </select>
            <div className="absolute inset-y-0 right-3 flex items-center pointer-events-none text-on-surface-variant">
              <svg width="10" height="6" viewBox="0 0 10 6" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M1.175 0.150024L5 3.97502L8.825 0.150024L10 1.32502L5 6.32502L0 1.32502L1.175 0.150024Z" fill="currentColor"/>
              </svg>
            </div>
          </div>
        </div>

        {/* Translation Result */}
        <section className="flex-1 flex flex-col min-h-0 bg-primary-container/10 border border-primary/20 rounded-xl p-4 shadow-sm">
          <div className="flex items-center justify-between mb-3 border-b border-primary/10 pb-2">
            <span className="text-[10px] uppercase font-bold tracking-[0.1em] text-primary">{t('widget.translation')}</span>
            <div className="flex gap-3">
              <button className="text-primary hover:text-primary-container transition-colors">
                <Volume2 size={16} />
              </button>
              <button
                type="button"
                disabled={!payload.translatedText}
                onClick={() => {
                  void handleCopyTranslation();
                }}
                className={`transition-colors ${payload.translatedText ? 'text-primary hover:text-primary-container' : 'cursor-not-allowed text-primary/40'} ${translationCopied ? 'text-primary-container' : ''}`}
              >
                {translationCopied ? (
                  <span className="text-[11px] font-bold">{t('history.copied')}</span>
                ) : (
                  <Copy size={16} />
                )}
              </button>
            </div>
          </div>
          <div className="flex-1 overflow-y-auto custom-scrollbar pr-2">
            {isTranslating && !payload.translatedText ? (
              <div className="flex items-center gap-2 text-primary">
                <Loader2 size={16} className="animate-spin" />
                <span className="text-sm">{t('widget.translating', { defaultValue: '翻译中...' })}</span>
              </div>
            ) : (
              <p className="font-body text-sm leading-relaxed text-primary-container font-medium">
                {standalone ? displayedTranslation : payload.translatedText}
                {isTypingTranslation && <span className="inline-block w-0.5 h-4 bg-primary ml-0.5 animate-pulse" />}
              </p>
            )}
          </div>
        </section>
      </main>

      {/* Footer */}
      <footer className="px-5 py-4 bg-white/90 border-t border-outline-variant/20 flex justify-end items-center shrink-0">
        <button
          type="button"
          disabled={isTranslateDisabled}
          onClick={() => {
            onTranslate?.();
          }}
          className={`px-6 py-2.5 rounded-lg text-sm font-bold flex items-center gap-2 transition-all ${
            isTranslateDisabled
              ? 'cursor-not-allowed bg-surface-container-high text-on-surface-variant shadow-none'
              : 'bg-primary text-white shadow-md hover:bg-primary-container active:scale-95'
          }`}
        >
          <ScanText size={16} />
          <span>
            {isLoading
              ? t('widget.waitForOcr', { defaultValue: '等待识别完成' })
              : t('widget.translate', { defaultValue: 'Translate' })}
          </span>
        </button>
      </footer>
    </div>
  );
}
