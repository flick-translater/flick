import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Clock3, Copy, FolderOpen, ImageIcon, RefreshCw, Trash2, X } from 'lucide-react';
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

type DeleteTarget =
  | { kind: 'single'; shot: CaptureRecord }
  | { kind: 'all' };

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

const ITEMS_PER_PAGE = 9;

export default function HistoryView() {
  const { t, i18n } = useTranslation();
  const [activeTab, setActiveTab] = useState<HistoryTab>('screenshots');
  const [history, setHistory] = useState<CaptureHistory>(emptyHistory);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [previewingShot, setPreviewingShot] = useState<CaptureRecord | null>(null);
  const [pendingDelete, setPendingDelete] = useState<DeleteTarget | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [currentPage, setCurrentPage] = useState(1);

  const loadHistory = async () => {
    try {
      setLoadError(null);
      const nextHistory = await invoke<CaptureHistory>('list_capture_history');
      setHistory(nextHistory);
    } catch (error) {
      setLoadError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    void loadHistory().catch(() => {});

    let unlisten: (() => void) | undefined;
    void listen<CaptureRecord>('capture-finished', async () => {
      if (!cancelled) {
        await loadHistory();
      }
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

  useEffect(() => {
    const totalPages = Math.max(1, Math.ceil(history.items.length / ITEMS_PER_PAGE));
    if (currentPage > totalPages) {
      setCurrentPage(totalPages);
    }
  }, [currentPage, history.items.length]);

  const formatter = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.language === 'zh' ? 'zh-CN' : i18n.language === 'ja' ? 'ja-JP' : 'en-US', {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [i18n.language],
  );

  const screenshotCountLabel = t('history.itemCount', { count: history.items.length });
  const totalPages = Math.max(1, Math.ceil(history.items.length / ITEMS_PER_PAGE));
  const pagedItems = history.items.slice((currentPage - 1) * ITEMS_PER_PAGE, currentPage * ITEMS_PER_PAGE);
  const pageStart = history.items.length === 0 ? 0 : (currentPage - 1) * ITEMS_PER_PAGE + 1;
  const pageEnd = Math.min(currentPage * ITEMS_PER_PAGE, history.items.length);
  const paginationItems = getPaginationItems(currentPage, totalPages);

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
            <section className="mb-6 rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-5 shadow-sm sm:mb-8 sm:p-6 lg:p-8">
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
                    onClick={() => setPendingDelete({ kind: 'all' })}
                    disabled={history.items.length === 0}
                    className="inline-flex items-center gap-2 rounded-full bg-error/10 px-3 py-1.5 text-xs font-bold text-error transition-colors hover:bg-error hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <Trash2 size={14} />
                    {t('history.deleteAll')}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setIsLoading(true);
                      void loadHistory();
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
              <>
                <div className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
                  {pagedItems.map((shot) => (
                  <ScreenshotCard
                    key={shot.id}
                    formatter={formatter}
                    shot={shot}
                    viewLabel={t('history.view')}
                    previewLabel={t('history.preview')}
                    deleteLabel={t('history.delete')}
                    copyPathLabel={t('history.copyPath')}
                    copyImageLabel={t('history.copyImage')}
                    copiedLabel={t('history.copied')}
                    onPreview={() => setPreviewingShot(shot)}
                    onDelete={() => setPendingDelete({ kind: 'single', shot })}
                  />
                ))}
                </div>
                {totalPages > 1 && (
                  <div className="mt-8 flex flex-col gap-4 border-t border-surface-container-high pt-6 sm:flex-row sm:items-center sm:justify-between">
                    <p className="text-xs font-bold uppercase tracking-wider text-on-surface-variant">
                      {t('history.showingRange', { start: pageStart, end: pageEnd, total: history.items.length })}
                    </p>
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        disabled={currentPage === 1}
                        onClick={() => setCurrentPage((page) => Math.max(1, page - 1))}
                        className="flex h-9 w-9 items-center justify-center rounded-lg bg-surface-container-lowest text-on-surface-variant ring-1 ring-outline-variant/30 transition-colors hover:text-primary disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        &lt;
                      </button>
                      {paginationItems.map((item, index) =>
                        item === 'ellipsis' ? (
                          <span
                            key={`ellipsis-${index}`}
                            className="flex h-9 w-9 items-center justify-center rounded-lg text-sm font-bold text-on-surface-variant"
                          >
                            ...
                          </span>
                        ) : (
                          <button
                            key={item}
                            type="button"
                            onClick={() => setCurrentPage(item)}
                            className={`flex h-9 w-9 items-center justify-center rounded-lg text-sm font-bold transition-colors ${
                              item === currentPage
                                ? 'bg-primary text-white'
                                : 'bg-surface-container-lowest text-on-surface ring-1 ring-outline-variant/30 hover:bg-surface-container'
                            }`}
                          >
                            {item}
                          </button>
                        ),
                      )}
                      <button
                        type="button"
                        disabled={currentPage === totalPages}
                        onClick={() => setCurrentPage((page) => Math.min(totalPages, page + 1))}
                        className="flex h-9 w-9 items-center justify-center rounded-lg bg-surface-container-lowest text-on-surface-variant ring-1 ring-outline-variant/30 transition-colors hover:text-primary disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        &gt;
                      </button>
                    </div>
                  </div>
                )}
              </>
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
          onDelete={() => setPendingDelete({ kind: 'single', shot: previewingShot })}
          shot={previewingShot}
          deleteLabel={t('history.delete')}
          viewLabel={t('history.view')}
        />
      )}

      {pendingDelete && (
        <ConfirmDeleteModal
          fileName={pendingDelete.kind === 'single' ? pendingDelete.shot.path.split(/[\\/]/).pop() ?? pendingDelete.shot.path : t('history.allScreenshots')}
          isDeleting={isDeleting}
          message={pendingDelete.kind === 'single' ? t('history.deleteConfirm') : t('history.deleteAllConfirm')}
          cancelLabel={t('history.cancel')}
          confirmLabel={pendingDelete.kind === 'single' ? t('history.delete') : t('history.deleteAll')}
          title={pendingDelete.kind === 'single' ? t('history.deleteTitle') : t('history.deleteAllTitle')}
          onCancel={() => {
            if (!isDeleting) {
              setPendingDelete(null);
            }
          }}
          onConfirm={() => {
            setIsDeleting(true);
            setLoadError(null);
            void (pendingDelete.kind === 'single'
              ? invoke('delete_capture', { path: pendingDelete.shot.path })
              : invoke('clear_all_captures'))
              .then(async () => {
                if (pendingDelete.kind === 'all' || (previewingShot && pendingDelete.kind === 'single' && previewingShot.path === pendingDelete.shot.path)) {
                  setPreviewingShot(null);
                }
                setPendingDelete(null);
                setIsLoading(true);
                setCurrentPage(1);
                await loadHistory();
              })
              .catch((error) => {
                setLoadError(error instanceof Error ? error.message : String(error));
              })
              .finally(() => {
                setIsDeleting(false);
              });
          }}
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
  deleteLabel,
  copyPathLabel,
  copyImageLabel,
  copiedLabel,
  onPreview,
  onDelete,
}: {
  shot: CaptureRecord;
  formatter: Intl.DateTimeFormat;
  viewLabel: string;
  previewLabel: string;
  deleteLabel: string;
  copyPathLabel: string;
  copyImageLabel: string;
  copiedLabel: string;
  onPreview: () => void;
  onDelete: () => void;
}) {
  const [pathCopied, setPathCopied] = useState(false);
  const [imageCopied, setImageCopied] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const imageUrl = useImageDataUrl(shot.path);
  const fileName = shot.path.split(/[\\/]/).pop() ?? shot.path;
  const isViewButtonHoverEnabled = useHoverEnabledAfterFocus();

  return (
    <article className="group overflow-hidden rounded-2xl border border-outline-variant/20 bg-surface-container-lowest shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-lg">
      <button
        type="button"
        onClick={onPreview}
        className="relative block aspect-[4/3] w-full overflow-hidden bg-surface-container text-left"
        title={previewLabel}
      >
        {imageUrl ? (
          <img
            src={imageUrl}
            alt={fileName}
            className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.03]"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-on-surface-variant">
            <ImageIcon size={28} />
          </div>
        )}
        <div className="absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-black/35 via-black/10 to-transparent" />
        <div className="absolute inset-0 bg-primary/0 transition-colors group-hover:bg-primary/5" />
      </button>
      <div className="space-y-3 p-4">
        <div>
          <p className="truncate text-[13px] font-bold text-on-surface">{fileName}</p>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] font-semibold text-on-surface-variant">
            <span className="rounded-full bg-surface-container px-2.5 py-1">{shot.width} x {shot.height}</span>
            <span className="inline-flex items-center gap-1.5 rounded-full bg-surface-container px-2.5 py-1">
              <Clock3 size={13} />
              {formatter.format(new Date(shot.created_at))}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2 text-[11px] font-semibold text-on-surface-variant">
          <span className="min-w-0 flex-1 truncate rounded-full bg-surface-container px-2.5 py-1" title={shot.path}>
            {shot.path}
          </span>
          <button
            type="button"
            onClick={() => {
              void navigator.clipboard.writeText(shot.path);
              setPathCopied(true);
              window.setTimeout(() => setPathCopied(false), 1500);
            }}
            className={`inline-flex h-8 shrink-0 items-center justify-center rounded-lg px-2 text-on-surface-variant transition-colors ${
              pathCopied ? 'bg-primary text-white hover:bg-primary-container hover:text-white' : 'bg-surface-container hover:text-on-surface'
            }`}
            title={copyPathLabel}
          >
            {pathCopied ? <span className="text-[11px] font-bold">{copiedLabel}</span> : <Copy size={16} />}
          </button>
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
            className={`flex-1 rounded-xl bg-surface-container px-3 py-2 text-[11px] font-bold text-on-surface transition-colors duration-200 ${
              isViewButtonHoverEnabled ? 'hover:bg-primary hover:text-white' : ''
            }`}
          >
            {viewLabel}
          </button>
          <button
            type="button"
            onClick={onDelete}
            className="inline-flex h-8 w-8 items-center justify-center rounded-lg text-on-surface-variant transition-colors hover:bg-error/10 hover:text-error"
            title={deleteLabel}
          >
            <Trash2 size={16} />
          </button>
          <button
            type="button"
            onClick={() => {
              setOpenError(null);
              void invoke('copy_capture_image', { path: shot.path })
                .then(() => {
                  setImageCopied(true);
                  window.setTimeout(() => setImageCopied(false), 1500);
                })
                .catch((error) => {
                  setOpenError(error instanceof Error ? error.message : String(error));
                });
            }}
            className={`inline-flex h-8 min-w-8 items-center justify-center rounded-lg px-2 text-on-surface-variant transition-colors ${
              imageCopied ? 'bg-primary text-white hover:bg-primary-container hover:text-white' : 'hover:bg-surface-container hover:text-on-surface'
            }`}
            title={copyImageLabel}
          >
            {imageCopied ? <span className="text-[11px] font-bold">{copiedLabel}</span> : <Copy size={16} />}
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
  deleteLabel,
  onClose,
  onDelete,
}: {
  shot: CaptureRecord;
  formatter: Intl.DateTimeFormat;
  viewLabel: string;
  deleteLabel: string;
  onClose: () => void;
  onDelete: () => void;
}) {
  const imageUrl = useImageDataUrl(shot.path);
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
              onClick={onDelete}
              className="flex h-8 w-8 items-center justify-center rounded-lg text-on-surface-variant transition-colors hover:bg-error/10 hover:text-error"
              title={deleteLabel}
            >
              <Trash2 size={16} />
            </button>
            <button
              type="button"
              onClick={() => {
                void invoke('open_file_in_default_app', { path: shot.path });
              }}
              className="flex h-8 w-8 items-center justify-center rounded-lg text-on-surface-variant transition-colors hover:bg-surface-container hover:text-on-surface"
              title={viewLabel}
            >
              <FolderOpen size={16} />
            </button>
            <button
              type="button"
              onClick={onClose}
              className="flex h-8 w-8 items-center justify-center rounded-lg text-on-surface-variant transition-colors hover:bg-surface-container hover:text-on-surface"
            >
              <X size={18} />
            </button>
          </div>
        </div>
        <div className="bg-[radial-gradient(circle_at_top,_rgba(0,41,117,0.12),_transparent_45%)] p-4 sm:p-6">
          <div className="overflow-hidden rounded-2xl bg-surface-container shadow-inner ring-1 ring-outline-variant/20">
            {imageUrl ? (
              <img src={imageUrl} alt={fileName} className="max-h-[72vh] w-full object-contain" />
            ) : (
              <div className="flex min-h-[320px] items-center justify-center text-on-surface-variant">
                <ImageIcon size={36} />
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function useImageDataUrl(path: string) {
  const [imageUrl, setImageUrl] = useState<string>('');

  useEffect(() => {
    let cancelled = false;

    void invoke<string>('read_image_as_data_url', { path })
      .then((dataUrl) => {
        if (!cancelled) {
          setImageUrl(dataUrl);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setImageUrl('');
        }
      });

    return () => {
      cancelled = true;
    };
  }, [path]);

  return imageUrl;
}

function getPaginationItems(currentPage: number, totalPages: number): Array<number | 'ellipsis'> {
  if (totalPages <= 7) {
    return Array.from({ length: totalPages }, (_, index) => index + 1);
  }

  if (currentPage <= 4) {
    return [1, 2, 3, 4, 5, 'ellipsis', totalPages];
  }

  if (currentPage >= totalPages - 3) {
    return [1, 'ellipsis', totalPages - 4, totalPages - 3, totalPages - 2, totalPages - 1, totalPages];
  }

  return [1, 'ellipsis', currentPage - 1, currentPage, currentPage + 1, 'ellipsis', totalPages];
}

function useHoverEnabledAfterFocus() {
  const [isHoverEnabled, setIsHoverEnabled] = useState(() => document.hasFocus());

  useEffect(() => {
    const handleFocus = () => setIsHoverEnabled(false);
    const handleBlur = () => setIsHoverEnabled(false);
    const handleMouseMove = () => setIsHoverEnabled(true);

    window.addEventListener('focus', handleFocus);
    window.addEventListener('blur', handleBlur);
    window.addEventListener('mousemove', handleMouseMove);

    return () => {
      window.removeEventListener('focus', handleFocus);
      window.removeEventListener('blur', handleBlur);
      window.removeEventListener('mousemove', handleMouseMove);
    };
  }, []);

  return isHoverEnabled;
}

function ConfirmDeleteModal({
  title,
  message,
  fileName,
  cancelLabel,
  confirmLabel,
  isDeleting,
  onCancel,
  onConfirm,
}: {
  title: string;
  message: string;
  fileName: string;
  cancelLabel: string;
  confirmLabel: string;
  isDeleting: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/55 p-4 backdrop-blur-sm" onClick={onCancel}>
      <div
        className="w-full max-w-md rounded-[28px] border border-outline-variant/20 bg-surface-container-lowest p-6 shadow-2xl"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-lg font-bold text-on-surface">{title}</p>
            <p className="mt-2 text-sm text-on-surface-variant">{message}</p>
            <p className="mt-3 break-all rounded-2xl bg-surface-container px-3 py-2 text-xs font-semibold text-on-surface">{fileName}</p>
          </div>
          <button
            type="button"
            onClick={onCancel}
            className="rounded-xl bg-surface-container p-2 text-on-surface-variant transition-colors hover:bg-surface-container-high hover:text-on-surface"
          >
            <X size={18} />
          </button>
        </div>
        <div className="mt-6 flex justify-end gap-3">
          <button
            type="button"
            disabled={isDeleting}
            onClick={onCancel}
            className="rounded-xl bg-surface-container px-4 py-2 text-sm font-bold text-on-surface transition-colors hover:bg-surface-container-high disabled:opacity-50"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            disabled={isDeleting}
            onClick={onConfirm}
            className="rounded-xl bg-error px-4 py-2 text-sm font-bold text-white transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {isDeleting ? '...' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
