import { useEffect, useRef, useState, type MouseEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Pin, Minus, X, Copy, ArrowRightLeft, Volume2, VolumeX, ScanText, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TranslationPayload } from '../types';
import { useTypewriter } from '../hooks/useTypewriter';

interface TranslationWindowProps {
  payload: TranslationPayload;
  isLoading?: boolean;
  isTranslating?: boolean;
  isSourceSpeaking?: boolean;
  isSourceSpeechLoading?: boolean;
  isTranslationSpeaking?: boolean;
  isTranslationSpeechLoading?: boolean;
  onClose: () => void;
  onTranslate?: () => void;
  onSwap?: () => void;
  onSourceSpeakToggle?: () => void;
  onTranslationSpeakToggle?: () => void;
  standalone?: boolean;
}

function normalizeDisplayText(text: string) {
  return text.replace(/\n{2,}/g, '\n');
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

export default function TranslationWindow({
  payload,
  isLoading = false,
  isTranslating = false,
  isSourceSpeaking = false,
  isSourceSpeechLoading = false,
  isTranslationSpeaking = false,
  isTranslationSpeechLoading = false,
  onClose,
  onTranslate,
  onSwap,
  onSourceSpeakToggle,
  onTranslationSpeakToggle,
  standalone = false,
}: TranslationWindowProps) {
  const { t } = useTranslation();
  const [isPinned, setIsPinned] = useState(false);
  const [pinningSupported, setPinningSupported] = useState(true);
  const [sourceCopied, setSourceCopied] = useState(false);
  const [translationCopied, setTranslationCopied] = useState(false);
  const isClosingRef = useRef(false);
  const interactionGuardRef = useRef<number | null>(null);
  const suppressBlurCloseRef = useRef(false);
  const pinnedRef = useRef(false);
  const windowHandle = standalone ? getCurrentWindow() : null;
  const isTranslateDisabled = isLoading || isTranslating || !payload.sourceText.trim();
  const isSourceSpeakDisabled = !payload.sourceText.trim();
  const isTranslationSpeakDisabled = !payload.translatedText.trim();
  const resolvedSourceLanguage = payload.detectedSourceLanguage?.toLowerCase() === 'auto'
    ? (payload.ocrDetectedSourceLanguage ?? 'auto')
    : (payload.detectedSourceLanguage ?? payload.ocrDetectedSourceLanguage);
  const sourceLanguageText = languageLabel(resolvedSourceLanguage, t);
  const targetLanguageText = languageLabel(payload.targetLanguage, t);
  const normalizedSourceText = normalizeDisplayText(payload.sourceText);
  const normalizedTranslatedText = normalizeDisplayText(payload.translatedText);

  const { displayedText: displayedSource, isTyping: isTypingSource } = useTypewriter(
    normalizedSourceText,
    { enabled: standalone }
  );

  const { displayedText: displayedTranslation, isTyping: isTypingTranslation } = useTypewriter(
    normalizedTranslatedText,
    { enabled: standalone }
  );

  useEffect(() => {
    if (!windowHandle) {
      return;
    }

    void Promise.all([
      invoke<boolean>('is_translate_window_pinning_supported'),
      invoke<boolean>('get_translate_window_pinned'),
    ])
      .then(([supported, pinned]) => {
        setPinningSupported(supported);
        pinnedRef.current = supported && pinned;
        setIsPinned(supported && pinned);
      })
      .catch((error) => {
        console.error('Failed to read translate window pinning state', error);
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
    suppressBlurCloseRef.current = true;
    if (interactionGuardRef.current !== null) {
      window.clearTimeout(interactionGuardRef.current);
    }
    interactionGuardRef.current = window.setTimeout(() => {
      suppressBlurCloseRef.current = false;
      interactionGuardRef.current = null;
    }, 300);

    try {
      await invoke('begin_translate_window_drag');
    } catch (error) {
      try {
        await windowHandle?.setFocus();
        await windowHandle?.startDragging();
      } catch (fallbackError) {
        console.error('Failed to start dragging translation window', fallbackError ?? error);
      }
    }
  };

  const handleTogglePinned = async () => {
    if (!pinningSupported) {
      return;
    }

    const next = !isPinned;
    const previous = isPinned;
    pinnedRef.current = next;
    setIsPinned(next);

    try {
      await invoke('set_translate_window_pinned', { pinned: next });
      pinnedRef.current = next;
      setIsPinned(next);
    } catch (error) {
      pinnedRef.current = previous;
      setIsPinned(previous);
      console.error('Failed to toggle always-on-top', error);
    }
  };

  const handleMinimize = async () => {
    try {
      await invoke('minimize_translate_window');
    } catch (error) {
      console.error('Failed to minimize translation window', error);
    }
  };

  const handleClose = async () => {
    if (isClosingRef.current) {
      return;
    }

    isClosingRef.current = true;

    try {
      if (standalone) {
        await invoke('close_translate_window');
        return;
      }
      onClose();
    } catch (error) {
      console.error('Failed to close translation window', error);
    } finally {
      window.setTimeout(() => {
        isClosingRef.current = false;
      }, 0);
    }
  };

  useEffect(() => {
    if (!standalone) {
      return;
    }

    const handleWindowBlur = () => {
      if (document.hasFocus() || pinnedRef.current || suppressBlurCloseRef.current) {
        return;
      }

      void handleClose();
    };

    window.addEventListener('blur', handleWindowBlur);

    return () => {
      if (interactionGuardRef.current !== null) {
        window.clearTimeout(interactionGuardRef.current);
      }
      window.removeEventListener('blur', handleWindowBlur);
    };
  }, [standalone]);

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
            disabled={!pinningSupported}
            onMouseDown={(event) => {
              event.stopPropagation();
            }}
            onClick={() => {
              void handleTogglePinned();
            }}
            title={pinningSupported ? undefined : 'Pinning is not supported in this Linux session'}
            className={`w-8 h-8 flex items-center justify-center rounded transition-colors ${
              !pinningSupported
                ? 'cursor-not-allowed text-outline-variant opacity-50'
                : isPinned
                  ? 'bg-primary text-white'
                  : 'text-on-surface-variant hover:bg-surface-container'
            }`}
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
              type="button"
              disabled={isSourceSpeakDisabled}
              onClick={() => {
                onSourceSpeakToggle?.();
              }}
              aria-label={isSourceSpeaking ? t('widget.stopReading', { defaultValue: '停止朗读' }) : t('widget.readSourceAloud', { defaultValue: '朗读原文' })}
              title={isSourceSpeaking ? t('widget.stopReading', { defaultValue: '停止朗读' }) : t('widget.readSourceAloud', { defaultValue: '朗读原文' })}
              className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-all duration-150 ${
                isSourceSpeakDisabled ? 'cursor-not-allowed text-outline-variant' : 'text-on-surface-variant hover:bg-primary/8 hover:text-primary active:scale-90'
              } ${(isSourceSpeaking || isSourceSpeechLoading) ? 'bg-primary/12 text-primary' : ''}`}
            >
              {isSourceSpeechLoading ? <Loader2 size={16} className="animate-spin" /> : isSourceSpeaking ? <VolumeX size={16} /> : <Volume2 size={16} />}
            </button>
            <button
              type="button"
              disabled={!payload.sourceText}
              onClick={() => {
                void handleCopySource();
              }}
              aria-label={t('common.copy', { defaultValue: 'Copy' })}
              title={t('common.copy', { defaultValue: 'Copy' })}
              className={`ml-auto flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-all duration-150 ${
                payload.sourceText ? 'text-on-surface-variant hover:bg-primary/8 hover:text-primary active:scale-90' : 'cursor-not-allowed text-outline-variant'
              } ${sourceCopied ? 'scale-110 bg-primary/12 text-primary' : ''}`}
            >
              <Copy size={16} />
            </button>
          </div>
          <div className="flex-1 overflow-y-auto custom-scrollbar pr-2">
            {isLoading && !payload.sourceText ? (
              <div className="flex items-center gap-2 text-on-surface-variant">
                <Loader2 size={16} className="animate-spin" />
                <span className="text-sm">{t('widget.recognizing', { defaultValue: '识别中...' })}</span>
              </div>
            ) : (
              <p className="whitespace-pre-wrap break-words font-body text-sm leading-relaxed text-on-surface">
                {standalone ? displayedSource : normalizedSourceText}
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
          
          <button
            type="button"
            disabled={!payload.sourceText.trim() && !payload.translatedText.trim()}
            onClick={() => {
              onSwap?.();
            }}
            className={`w-10 h-10 rounded-lg flex items-center justify-center shadow-md transition-all active:scale-95 shrink-0 ${
              payload.sourceText.trim() || payload.translatedText.trim()
                ? 'bg-primary text-white hover:bg-primary-container'
                : 'cursor-not-allowed bg-surface-container-high text-on-surface-variant shadow-none'
            }`}
          >
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
              <button
                type="button"
                disabled={isTranslationSpeakDisabled}
                onClick={() => {
                  onTranslationSpeakToggle?.();
                }}
                aria-label={isTranslationSpeaking ? t('widget.stopReading', { defaultValue: '停止朗读' }) : t('widget.readAloud', { defaultValue: '朗读译文' })}
                title={isTranslationSpeaking ? t('widget.stopReading', { defaultValue: '停止朗读' }) : t('widget.readAloud', { defaultValue: '朗读译文' })}
                className={`flex h-8 w-8 items-center justify-center rounded-md transition-all duration-150 ${
                  isTranslationSpeakDisabled ? 'cursor-not-allowed text-primary/40' : 'text-primary hover:bg-primary/10 hover:text-primary-container active:scale-90'
                } ${(isTranslationSpeaking || isTranslationSpeechLoading) ? 'bg-primary/12 text-primary-container' : ''}`}
              >
                {isTranslationSpeechLoading ? <Loader2 size={16} className="animate-spin" /> : isTranslationSpeaking ? <VolumeX size={16} /> : <Volume2 size={16} />}
              </button>
              <button
                type="button"
                disabled={!payload.translatedText}
                onClick={() => {
                  void handleCopyTranslation();
                }}
                aria-label={t('common.copy', { defaultValue: 'Copy' })}
                title={t('common.copy', { defaultValue: 'Copy' })}
                className={`flex h-8 w-8 items-center justify-center rounded-md transition-all duration-150 ${
                  payload.translatedText ? 'text-primary hover:bg-primary/10 hover:text-primary-container active:scale-90' : 'cursor-not-allowed text-primary/40'
                } ${translationCopied ? 'scale-110 bg-primary/12 text-primary-container' : ''}`}
              >
                <Copy size={16} />
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
              <p className="whitespace-pre-wrap break-words font-body text-sm leading-relaxed text-primary-container font-medium">
                {standalone ? displayedTranslation : normalizedTranslatedText}
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
