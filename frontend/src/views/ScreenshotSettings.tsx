import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Download, Image, LoaderCircle, Paintbrush, Video } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';
import { AppSettings, FfmpegStatus } from '../types';

type GifSize = '540p' | '720p';
type GifFps = 6 | 8 | 10;
type VideoSize = '540p' | '720p' | '1080p';
type VideoFps = 24 | 30;

type FfmpegDownloadProgress = {
  downloaded: number;
  total?: number | null;
  percent?: number | null;
};

const missingFfmpeg: FfmpegStatus = {
  available: false,
  path: '',
  source: 'missing',
};

export default function ScreenshotSettings() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [ffmpegStatus, setFfmpegStatus] = useState<FfmpegStatus>(missingFfmpeg);
  const [isDownloadingFfmpeg, setIsDownloadingFfmpeg] = useState(false);
  const [ffmpegDownloadPercent, setFfmpegDownloadPercent] = useState<number | null>(null);
  const [error, setError] = useState('');
  const isLinux = useMemo(() => /Linux/i.test(navigator.platform), []);
  const gifSize = normalizeGifSize(settings?.gif_recording_size);
  const gifFps = normalizeGifFps(settings?.gif_recording_fps);
  const videoSize = normalizeVideoSize(settings?.video_recording_size);
  const videoFps = normalizeVideoFps(settings?.video_recording_fps);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<FfmpegDownloadProgress>('ffmpeg-download-progress', (event) => {
      if (disposed) {
        return;
      }
      const percent = event.payload.percent;
      setFfmpegDownloadPercent(typeof percent === 'number' ? clampPercent(percent) : null);
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    void Promise.all([
      invoke<AppSettings>('get_app_settings'),
      invoke<FfmpegStatus>('get_ffmpeg_status'),
    ])
      .then(([appSettings, status]) => {
        setSettings(appSettings);
        setFfmpegStatus(status);
        setError('');
      })
      .catch((loadError: unknown) => {
        setError(String(loadError));
      });
  }, []);

  function updateGifSize(size: GifSize) {
    setSettings((current) => current ? { ...current, gif_recording_size: size } : current);
    void invoke<AppSettings>('update_gif_recording_size', { size })
      .then((updated) => {
        setSettings(updated);
        setError('');
      })
      .catch((updateError: unknown) => {
        setSettings((current) => current ? { ...current, gif_recording_size: gifSize } : current);
        setError(String(updateError));
      });
  }

  function updateGifFps(fps: GifFps) {
    setSettings((current) => current ? { ...current, gif_recording_fps: fps } : current);
    void invoke<AppSettings>('update_gif_recording_fps', { fps })
      .then((updated) => {
        setSettings(updated);
        setError('');
      })
      .catch((updateError: unknown) => {
        setSettings((current) => current ? { ...current, gif_recording_fps: gifFps } : current);
        setError(String(updateError));
      });
  }

  function updateVideoSize(size: VideoSize) {
    setSettings((current) => current ? { ...current, video_recording_size: size } : current);
    void invoke<AppSettings>('update_video_recording_size', { size })
      .then((updated) => {
        setSettings(updated);
        setError('');
      })
      .catch((updateError: unknown) => {
        setSettings((current) => current ? { ...current, video_recording_size: videoSize } : current);
        setError(String(updateError));
      });
  }

  function updateVideoFps(fps: VideoFps) {
    setSettings((current) => current ? { ...current, video_recording_fps: fps } : current);
    void invoke<AppSettings>('update_video_recording_fps', { fps })
      .then((updated) => {
        setSettings(updated);
        setError('');
      })
      .catch((updateError: unknown) => {
        setSettings((current) => current ? { ...current, video_recording_fps: videoFps } : current);
        setError(String(updateError));
      });
  }

  function downloadFfmpeg() {
    setIsDownloadingFfmpeg(true);
    setFfmpegDownloadPercent(0);
    setError('');
    void invoke<FfmpegStatus>('download_ffmpeg')
      .then((status) => {
        setFfmpegStatus(status);
        setFfmpegDownloadPercent(100);
      })
      .catch((downloadError: unknown) => {
        setError(String(downloadError));
      })
      .finally(() => {
        setIsDownloadingFfmpeg(false);
        window.setTimeout(() => setFfmpegDownloadPercent(null), 600);
      });
  }

  return (
    <div className="mx-auto max-w-5xl animate-in fade-in duration-500">
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 lg:gap-4">
        <section className="rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-4 shadow-sm transition-shadow duration-300 hover:shadow-md lg:col-span-2">
          <div className="mb-3 flex items-start justify-between gap-4">
            <div>
              <h2 className="mb-0.5 font-headline text-base font-bold text-primary">{t('screenshotSettings.captureBehavior')}</h2>
              <p className="text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.captureBehaviorDesc')}</p>
            </div>
            <div className="rounded-lg bg-primary/5 p-2 text-primary">
              <Image size={20} />
            </div>
          </div>

          {!isLinux && (
            <div className="flex items-center justify-between gap-4 rounded-lg bg-surface-container-low p-3">
              <div>
                <h3 className="text-sm font-bold text-on-surface">{t('screenshotSettings.screenshotEditorToolbar')}</h3>
                <p className="mt-0.5 text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.screenshotEditorToolbarDesc')}</p>
              </div>
              <div className="flex items-center gap-3">
                <Paintbrush size={18} className="text-primary" />
                <Toggle
                  checked={settings?.screenshot_editor_toolbar_enabled ?? true}
                  onChange={(enabled) => {
                    setSettings((current) => current ? { ...current, screenshot_editor_toolbar_enabled: enabled } : current);
                    void invoke<AppSettings>('update_screenshot_editor_toolbar_enabled', { enabled })
                      .then((updated) => {
                        setSettings(updated);
                        setError('');
                      })
                      .catch((updateError: unknown) => {
                        setSettings((current) => current ? { ...current, screenshot_editor_toolbar_enabled: !enabled } : current);
                        setError(String(updateError));
                      });
                  }}
                />
              </div>
            </div>
          )}
        </section>

        <section className="rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-4 shadow-sm transition-shadow duration-300 hover:shadow-md lg:col-span-2">
          <div className="mb-3 flex items-start justify-between gap-4">
            <div>
              <h2 className="mb-0.5 font-headline text-base font-bold text-primary">{t('screenshotSettings.gifRecording')}</h2>
              <p className="text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.gifRecordingDesc')}</p>
            </div>
            <div className="rounded-lg bg-primary/5 p-2 text-primary">
              <Video size={20} />
            </div>
          </div>

          <div className="space-y-3 rounded-lg bg-surface-container-low p-3">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <h3 className="text-sm font-bold text-on-surface">{t('screenshotSettings.gifSize')}</h3>
                <p className="mt-0.5 text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.gifSizeDesc')}</p>
              </div>
              <div className="inline-flex shrink-0 rounded-lg border border-outline-variant/30 bg-surface-container p-1">
                {(['540p', '720p'] as GifSize[]).map((size) => (
                  <button
                    key={size}
                    type="button"
                    onClick={() => updateGifSize(size)}
                    className={`min-w-20 rounded-md px-3 py-1.5 text-xs font-bold transition-colors ${
                      gifSize === size
                        ? 'bg-primary text-white shadow-sm'
                        : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
                    }`}
                  >
                    {size}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <h3 className="text-sm font-bold text-on-surface">{t('screenshotSettings.gifFps')}</h3>
                <p className="mt-0.5 text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.gifFpsDesc')}</p>
              </div>
              <div className="inline-flex shrink-0 rounded-lg border border-outline-variant/30 bg-surface-container p-1">
                {([6, 8, 10] as GifFps[]).map((fps) => (
                  <button
                    key={fps}
                    type="button"
                    onClick={() => updateGifFps(fps)}
                    className={`min-w-20 rounded-md px-3 py-1.5 text-xs font-bold transition-colors ${
                      gifFps === fps
                        ? 'bg-primary text-white shadow-sm'
                        : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
                    }`}
                  >
                    {fps} FPS
                  </button>
                ))}
              </div>
            </div>
          </div>
        </section>

        <section className="rounded-xl border border-outline-variant/20 bg-surface-container-lowest p-4 shadow-sm transition-shadow duration-300 hover:shadow-md lg:col-span-2">
          <div className="mb-3 flex items-start justify-between gap-4">
            <div>
              <h2 className="mb-0.5 font-headline text-base font-bold text-primary">{t('screenshotSettings.videoRecording')}</h2>
              <p className="text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.videoRecordingDesc')}</p>
            </div>
            <div className="rounded-lg bg-primary/5 p-2 text-primary">
              <Video size={20} />
            </div>
          </div>

          <div className="space-y-3 rounded-lg bg-surface-container-low p-3">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <h3 className="text-sm font-bold text-on-surface">{t('screenshotSettings.ffmpegStatus')}</h3>
                <p className="mt-0.5 break-all text-xs leading-relaxed text-on-surface-variant">
                  {ffmpegStatus.available
                    ? t('screenshotSettings.ffmpegAvailable', { path: ffmpegStatus.path })
                    : t('screenshotSettings.ffmpegMissing')}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <button
                  type="button"
                  disabled={ffmpegStatus.available || isDownloadingFfmpeg}
                  onClick={downloadFfmpeg}
                  className="inline-flex h-9 shrink-0 items-center gap-2 rounded-lg bg-primary px-3 text-xs font-bold text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:bg-surface-container-highest disabled:text-on-surface-variant"
                >
                  {isDownloadingFfmpeg ? <LoaderCircle size={14} className="animate-spin" /> : <Download size={14} />}
                  {isDownloadingFfmpeg ? t('screenshotSettings.downloadingFfmpeg') : t('screenshotSettings.downloadFfmpeg')}
                </button>
                {isDownloadingFfmpeg && (
                  <DownloadProgressCircle percent={ffmpegDownloadPercent} />
                )}
              </div>
            </div>

            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <h3 className="text-sm font-bold text-on-surface">{t('screenshotSettings.videoSize')}</h3>
                <p className="mt-0.5 text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.videoSizeDesc')}</p>
              </div>
              <div className="inline-flex shrink-0 rounded-lg border border-outline-variant/30 bg-surface-container p-1">
                {(['540p', '720p', '1080p'] as VideoSize[]).map((size) => (
                  <button
                    key={size}
                    type="button"
                    disabled={!ffmpegStatus.available}
                    onClick={() => updateVideoSize(size)}
                    className={`min-w-20 rounded-md px-3 py-1.5 text-xs font-bold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                      videoSize === size
                        ? 'bg-primary text-white shadow-sm'
                        : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
                    }`}
                  >
                    {size}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <h3 className="text-sm font-bold text-on-surface">{t('screenshotSettings.videoFps')}</h3>
                <p className="mt-0.5 text-xs leading-relaxed text-on-surface-variant">{t('screenshotSettings.videoFpsDesc')}</p>
              </div>
              <div className="inline-flex shrink-0 rounded-lg border border-outline-variant/30 bg-surface-container p-1">
                {([24, 30] as VideoFps[]).map((fps) => (
                  <button
                    key={fps}
                    type="button"
                    disabled={!ffmpegStatus.available}
                    onClick={() => updateVideoFps(fps)}
                    className={`min-w-20 rounded-md px-3 py-1.5 text-xs font-bold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                      videoFps === fps
                        ? 'bg-primary text-white shadow-sm'
                        : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
                    }`}
                  >
                    {fps} FPS
                  </button>
                ))}
              </div>
            </div>
          </div>
        </section>
      </div>

      {error && <p className="mt-4 text-xs text-error">{error}</p>}
    </div>
  );
}

function normalizeGifSize(value?: string): GifSize {
  return value === '540p' ? '540p' : '720p';
}

function normalizeGifFps(value?: number): GifFps {
  return value === 8 || value === 10 ? value : 6;
}

function normalizeVideoSize(value?: string): VideoSize {
  return value === '540p' || value === '1080p' ? value : '720p';
}

function normalizeVideoFps(value?: number): VideoFps {
  return value === 30 ? 30 : 24;
}

function clampPercent(value: number) {
  return Math.max(0, Math.min(100, Math.round(value)));
}

function DownloadProgressCircle({ percent }: { percent: number | null }) {
  const radius = 14;
  const circumference = 2 * Math.PI * radius;
  const normalized = percent ?? 0;
  const dashOffset = circumference * (1 - normalized / 100);

  return (
    <div className="relative h-9 w-9 shrink-0" aria-label={percent === null ? undefined : `${normalized}%`}>
      <svg className="h-9 w-9 -rotate-90" viewBox="0 0 36 36" aria-hidden="true">
        <circle
          cx="18"
          cy="18"
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          className="text-outline-variant/35"
        />
        <circle
          cx="18"
          cy="18"
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          className={percent === null ? 'animate-pulse text-primary' : 'text-primary transition-[stroke-dashoffset] duration-150'}
        />
      </svg>
      {percent !== null && (
        <span className="absolute inset-0 flex items-center justify-center text-[10px] font-bold text-primary">
          {normalized}
        </span>
      )}
    </div>
  );
}
