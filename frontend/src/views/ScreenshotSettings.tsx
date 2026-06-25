import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Image, Paintbrush, Video } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import Toggle from '../components/Toggle';
import { AppSettings } from '../types';

type GifSize = '540p' | '720p';
type GifFps = 6 | 8 | 10;

export default function ScreenshotSettings() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [error, setError] = useState('');
  const isLinux = useMemo(() => /Linux/i.test(navigator.platform), []);
  const gifSize = normalizeGifSize(settings?.gif_recording_size);
  const gifFps = normalizeGifFps(settings?.gif_recording_fps);

  useEffect(() => {
    void invoke<AppSettings>('get_app_settings')
      .then((appSettings) => {
        setSettings(appSettings);
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
