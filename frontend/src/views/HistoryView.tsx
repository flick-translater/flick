import { useEffect, useMemo, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Clock3, Copy, FolderOpen, ImageIcon, RefreshCw, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { CaptureHistory, CaptureRecord } from '../types';

type HistoryTab = 'screenshots' | 'translations';

type TranslationMockRecord = {
  id: number;
  sourceLang: string;
  targetLang: string;
  sourceText: string;
  targetText: string;
  time: string;
  highlight?: boolean;
};

const emptyHistory: CaptureHistory = {
  directory: '',
  items: [],
};

const translationMocks: TranslationMockRecord[] = [
  {
    id: 1,
    sourceLang: 'Japanese',
    targetLang: 'English',
    sourceText: '明日の会議の資料を、午後三時までに共有してください。',
    targetText: "Please share the materials for tomorrow's meeting by 3:00 PM.",
    time: 'Today, 10:45 AM',
  },
  {
    id: 2,
    sourceLang: 'German',
    targetLang: 'English',
    sourceText: 'Die Effizienz des Systems wurde durch die neuen AI-Modelle erheblich gesteigert.',
    targetText: 'The efficiency of the system has been significantly increased by the new AI models.',
    time: 'Yesterday, 4:12 PM',
  },
  {
    id: 3,
    sourceLang: 'Spanish',
    targetLang: 'English',
    sourceText: 'El diseño de la interfaz debe ser tanto funcional como estéticamente agradable.',
    targetText: 'The design of the interface must be both functional and aesthetically pleasing.',
    time: 'Oct 12, 09:20 AM',
    highlight: true,
  },
];

export default function HistoryView() {
  const { t, i18n } = useTranslation();
  const [activeTab, setActiveTab] = useState<HistoryTab>('screenshots');
  const [history, setHistory] = useState<CaptureHistory>(emptyHistory);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [previewingShot, setPreviewingShot] = useState<CaptureRecord | null>(null);

  useEffect(() => {
    let cancelled = false;

    const loadHistory = async () => {
      try {
        setLoadError(null);
        const nextHistory = await invoke<CaptureHistory>('list_capture_history');
        if (!cancelled) {
          setHistory(nextHistory);
        }
      } catch (error) {
        if (!cancelled) {
          setLoadError(error instanceof Error ? error.message : String(error));
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    };

    void loadHistory();

    let unlisten: (() => void) | undefined;
    void listen<CaptureRecord>('capture-finished', async () => {
      await loadHistory();
    }).then((dispose) => {
      unlisten = dispose;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!previewingShot) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setPreviewingShot(null);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [previewingShot]);

  const formatter = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.language === 'zh' ? 'zh-CN' : i18n.language === 'ja' ? 'ja-JP' : 'en-US', {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [i18n.language],
  );

  const screenshotCountLabel = t('history.itemCount', { count: history.items.length });

  return (
    <>
      <div className="mx-auto max-w-6xl animate-in fade-in duration-500">
        <div className="mb-6 flex w-full items-center gap-2 overflow-x-auto rounded-2xl border border-outline-variant/20 bg-surface-container p-1.5 shadow-sm sm:mb-8 md:w-fit lg:mb-10">
          <button
            onClick={() => setActiveTab('screenshots')}
            className={`rounded-xl px-6 py-2.5 text-sm font-bold transition-all ${
              activeTab === 'screenshots'
                ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5'
                : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
            }`}
          >
            {t('history.screenshotHistory')}
          </button>
          <button
            onClick={() => setActiveTab('translations')}
            className={`rounded-xl px-6 py-2.5 text-sm font-bold transition-all ${
              activeTab === 'translations'
                ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5'
                : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
            }`}
          >
            {t('history.translationHistory')}
          </button>
        </div>

        {activeTab === 'screenshots' ? (
          <>
            <section className="mb-6 rounded-3xl border border-outline-variant/20 bg-gradient-to-br from-white via-surface-container-lowest to-surface-container p-5 shadow-sm sm:mb-8 sm:p-6">
              <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
                <div className="space-y-2">
                  <p className="text-xs font-bold uppercase tracking-[0.24em] text-primary/70">{t('history.storageDirectory')}</p>
                  <p className="break-all font-mono text-sm text-on-surface">{history.directory || t('history.notAvailable')}</p>
                </div>
                <div className="flex items-center gap-3 text-sm text-on-surface-variant">
                  <span className="inline-flex items-center gap-2 rounded-full bg-white/80 px-3 py-1.5 ring-1 ring-outline-variant/20">
                    <FolderOpen size={15} />
                    {screenshotCountLabel}
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      setIsLoading(true);
                      void invoke<CaptureHistory>('list_capture_history')
                        .then((nextHistory) => {
                          setHistory(nextHistory);
                          setLoadError(null);
                        })
                        .catch((error) => {
                          setLoadError(error instanceof Error ? error.message : String(error));
                        })
                        .finally(() => {
                          setIsLoading(false);
                        });
                    }}
                    className="inline-flex items-center gap-2 rounded-full bg-primary px-3 py-1.5 text-xs font-bold text-white transition-opacity hover:opacity-90"
                  >
                    <RefreshCw size={14} />
                    {t('history.refresh')}
                  </button>
                </div>
              </div>
            </section>

            {isLoading ? (
              <div className="rounded-3xl border border-outline-variant/20 bg-surface-container-lowest p-10 text-center text-sm text-on-surface-variant">
                {t('history.loading')}
              </div>
            ) : loadError ? (
              <div className="rounded-3xl border border-error/20 bg-error/5 p-10 text-center">
                <p className="text-sm font-semibold text-error">{t('history.loadFailed')}</p>
                <p className="mt-2 break-all text-xs text-on-surface-variant">{loadError}</p>
              </div>
            ) : history.items.length === 0 ? (
              <div className="rounded-3xl border border-dashed border-outline-variant/30 bg-surface-container-lowest p-12 text-center">
                <div className="mx-auto flex h-14 w-14 items-center justify-center rounded-2xl bg-primary/8 text-primary">
                  <ImageIcon size={24} />
                </div>
                <p className="mt-4 text-base font-bold text-on-surface">{t('history.emptyTitle')}</p>
                <p className="mt-2 text-sm text-on-surface-variant">{t('history.emptyDesc')}</p>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 sm:gap-5 xl:grid-cols-3 xl:gap-6">
                {history.items.map((shot) => (
                  <ScreenshotCard
                    key={shot.id}
                    formatter={formatter}
                    shot={shot}
                    viewLabel={t('history.view')}
                    previewLabel={t('history.preview')}
                    copyPathLabel={t('history.copyPath')}
                    copiedLabel={t('history.copied')}
                    onPreview={() => setPreviewingShot(shot)}
                  />
                ))}
              </div>
            )}
          </>
        ) : (
          <div className="space-y-6">
            {translationMocks.map((trans) => (
              <article
                key={trans.id}
                className={`group relative flex flex-col items-start gap-5 rounded-xl border p-5 shadow-sm transition-all sm:p-6 md:flex-row md:gap-8 ${
                  trans.highlight
                    ? 'border-primary/20 bg-gradient-to-br from-primary/5 to-white'
                    : 'border-outline-variant/20 bg-surface-container-lowest hover:shadow-md'
                }`}
              >
                <div className="flex-1 space-y-4">
                  <div className="flex items-center gap-3">
                    <span className={`inline-flex items-center rounded-md px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${trans.highlight ? 'bg-primary text-white' : 'bg-primary/10 text-primary'}`}>
                      {trans.sourceLang}
                    </span>
                    <span className="text-sm text-outline">→</span>
                    <span className="inline-flex items-center rounded-md bg-surface-container px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-on-surface-variant">
                      {trans.targetLang}
                    </span>
                  </div>
                  <div className="space-y-3">
                    <p className="text-lg font-bold leading-snug text-on-surface">{trans.sourceText}</p>
                    <p className="text-lg font-medium leading-snug text-primary">{trans.targetText}</p>
                  </div>
                </div>
                <div className="flex w-full flex-row items-center justify-between gap-3 md:w-auto md:shrink-0 md:flex-col md:items-end">
                  <span className="flex items-center gap-1.5 text-xs font-semibold text-on-surface-variant">
                    <Clock3 size={14} />
                    {trans.time}
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      void navigator.clipboard.writeText(trans.targetText);
                    }}
                    className="rounded-lg bg-surface-container p-2 text-on-surface-variant transition-colors hover:bg-primary-container hover:text-white"
                    title={t('history.copyTranslation')}
                  >
                    <Copy size={16} />
                  </button>
                </div>
              </article>
            ))}
          </div>
        )}
      </div>

      {previewingShot && (
        <PreviewModal
          formatter={formatter}
          onClose={() => setPreviewingShot(null)}
          shot={previewingShot}
          viewLabel={t('history.view')}
        />
      )}
    </>
  );
}

function ScreenshotCard({
  shot,
  formatter,
  viewLabel,
  previewLabel,
  copyPathLabel,
  copiedLabel,
  onPreview,
}: {
  shot: CaptureRecord;
  formatter: Intl.DateTimeFormat;
  viewLabel: string;
  previewLabel: string;
  copyPathLabel: string;
  copiedLabel: string;
  onPreview: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const imageUrl = convertFileSrc(shot.path);
  const fileName = shot.path.split(/[\\/]/).pop() ?? shot.path;

  return (
    <article className="group overflow-hidden rounded-3xl border border-outline-variant/20 bg-surface-container-lowest shadow-sm transition-all hover:-translate-y-1 hover:shadow-xl">
      <button
        type="button"
        onClick={onPreview}
        className="relative block aspect-[16/10] w-full overflow-hidden bg-surface-container text-left"
        title={previewLabel}
      >
        <img
          src={imageUrl}
          alt={fileName}
          className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.03]"
        />
        <div className="absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-black/35 via-black/10 to-transparent" />
        <div className="absolute inset-0 bg-primary/0 transition-colors group-hover:bg-primary/5" />
      </button>
      <div className="space-y-4 p-5">
        <div>
          <p className="truncate text-sm font-bold text-on-surface">{fileName}</p>
          <p className="mt-1 flex items-center gap-1.5 text-xs text-on-surface-variant">
            <Clock3 size={13} />
            {formatter.format(new Date(shot.created_at))}
          </p>
        </div>

        <div className="flex flex-wrap gap-2 text-[11px] font-semibold text-on-surface-variant">
          <span className="rounded-full bg-surface-container px-2.5 py-1">{shot.width} x {shot.height}</span>
          <span className="max-w-full truncate rounded-full bg-surface-container px-2.5 py-1" title={shot.path}>
            {shot.path}
          </span>
        </div>

        <div className="flex gap-2 border-t border-outline-variant/20 pt-3">
          <button
            type="button"
            onClick={() => {
              setOpenError(null);
              void invoke('open_file_in_default_app', { path: shot.path }).catch((error) => {
                setOpenError(error instanceof Error ? error.message : String(error));
              });
            }}
            className="flex-1 rounded-xl bg-primary px-3 py-2.5 text-xs font-bold text-white transition-opacity hover:opacity-90"
          >
            {viewLabel}
          </button>
          <button
            type="button"
            onClick={() => {
              void navigator.clipboard.writeText(shot.path);
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            }}
            className="inline-flex items-center justify-center rounded-xl bg-surface-container px-3 py-2.5 text-on-surface-variant transition-colors hover:bg-surface-container-high hover:text-on-surface"
            title={copyPathLabel}
          >
            {copied ? <span className="text-[11px] font-bold text-primary">{copiedLabel}</span> : <Copy size={16} />}
          </button>
        </div>
        {openError && <p className="text-xs text-error">{openError}</p>}
      </div>
    </article>
  );
}

function PreviewModal({
  shot,
  formatter,
  viewLabel,
  onClose,
}: {
  shot: CaptureRecord;
  formatter: Intl.DateTimeFormat;
  viewLabel: string;
  onClose: () => void;
}) {
  const imageUrl = convertFileSrc(shot.path);
  const fileName = shot.path.split(/[\\/]/).pop() ?? shot.path;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/72 p-4 backdrop-blur-sm" onClick={onClose}>
      <div
        className="w-full max-w-5xl overflow-hidden rounded-[28px] bg-surface-container-lowest shadow-2xl"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-outline-variant/20 px-5 py-4 sm:px-6">
          <div className="min-w-0">
            <p className="truncate text-sm font-bold text-on-surface">{fileName}</p>
            <p className="mt-1 text-xs text-on-surface-variant">{formatter.format(new Date(shot.created_at))}</p>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => {
                void invoke('open_file_in_default_app', { path: shot.path });
              }}
              className="rounded-xl bg-primary px-3 py-2 text-xs font-bold text-white transition-opacity hover:opacity-90"
            >
              {viewLabel}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded-xl bg-surface-container p-2 text-on-surface-variant transition-colors hover:bg-surface-container-high hover:text-on-surface"
            >
              <X size={18} />
            </button>
          </div>
        </div>
        <div className="bg-[radial-gradient(circle_at_top,_rgba(0,41,117,0.12),_transparent_45%)] p-4 sm:p-6">
          <div className="overflow-hidden rounded-2xl bg-surface-container shadow-inner ring-1 ring-outline-variant/20">
            <img src={imageUrl} alt={fileName} className="max-h-[72vh] w-full object-contain" />
          </div>
        </div>
      </div>
    </div>
  );
}
