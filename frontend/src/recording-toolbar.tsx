import React, { useMemo, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Pause, Play, Square, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import './index.css';
import { normalizeLanguage, setupI18n } from './i18n/config';
import type { AppSettings } from './types';

type RecordingStatus = 'idle' | 'recording' | 'paused' | 'saving';
type RecordingFormat = 'gif' | 'video';
const toolbarTooltipClass = 'pointer-events-none absolute left-1/2 z-[80] -translate-x-1/2 whitespace-nowrap rounded-md bg-surface-container-highest px-2 py-1 text-xs font-semibold text-on-surface opacity-0 shadow-lg ring-1 ring-outline-variant/30 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100';

function RecordingToolbar() {
  const { t } = useTranslation();
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const sessionId = query.get('session_id') ?? '';
  const [status, setStatus] = useState<RecordingStatus>('idle');
  const [format, setFormat] = useState<RecordingFormat>('gif');
  const [error, setError] = useState('');
  const appWindow = useMemo(() => getCurrentWindow(), []);

  async function startOrResume() {
    if (!sessionId || status === 'recording' || status === 'saving') {
      return;
    }
    setError('');
    try {
      await invoke('set_recording_window_mode', { sessionId, recording: true });
      if (status === 'paused') {
        await invoke('resume_recording', { sessionId });
      } else {
        await invoke('start_recording', { sessionId, format });
      }
      setStatus('recording');
    } catch (err) {
      setError(String(err));
      setStatus('idle');
    }
  }

  async function pause() {
    if (!sessionId || status !== 'recording') {
      return;
    }
    setError('');
    try {
      await invoke('pause_recording', { sessionId });
      setStatus('paused');
    } catch (err) {
      setError(String(err));
    }
  }

  async function finish() {
    if (!sessionId || status === 'idle' || status === 'saving') {
      return;
    }
    setError('');
    setStatus('saving');
    try {
      await invoke('finish_recording', { sessionId });
      await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
      await appWindow.close();
    } catch (err) {
      setStatus('paused');
      setError(String(err));
    }
  }

  async function cancel() {
    if (!sessionId) {
      await appWindow.close();
      return;
    }
    await invoke('cancel_recording', { sessionId }).catch(() => undefined);
    await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
    await appWindow.close();
  }

  const disabled = status === 'saving';
  const canStart = !disabled && status !== 'recording';
  const canPause = !disabled && status === 'recording';
  const canFinish = !disabled && status !== 'idle';
  const startLabel = status === 'paused'
    ? t('screenshotEditor.actions.resumeRecording')
    : t('screenshotEditor.actions.startRecording');
  const pauseLabel = t('screenshotEditor.actions.pauseRecording');
  const finishLabel = t('screenshotEditor.actions.finishRecording');
  const cancelLabel = t('screenshotEditor.actions.cancel');

  return (
    <div className="flex h-screen items-end justify-center bg-transparent p-1 text-on-surface">
      <div className="flex items-center gap-1.5 rounded-lg border border-outline-variant/30 bg-surface-container-lowest/95 p-1.5 shadow-xl backdrop-blur">
        <TooltipButton label={startLabel}>
          <button
            type="button"
            aria-label={startLabel}
            disabled={!canStart}
            onClick={() => void startOrResume()}
            className="flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Play size={18} />
          </button>
        </TooltipButton>
        <div className="mx-1 h-6 w-px bg-outline-variant/40" />
        <div className="inline-flex rounded-md border border-outline-variant/30 bg-surface-container p-0.5">
          {(['gif', 'video'] as RecordingFormat[]).map((item) => (
            <button
              key={item}
              type="button"
              disabled={status !== 'idle'}
              onClick={() => setFormat(item)}
              className={`h-7 rounded px-2 text-[11px] font-bold transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${
                format === item
                  ? 'bg-primary text-white'
                  : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
              }`}
            >
              {item === 'gif'
                ? t('screenshotEditor.actions.recordingFormatGif')
                : t('screenshotEditor.actions.recordingFormatVideo')}
            </button>
          ))}
        </div>
        <TooltipButton label={pauseLabel}>
          <button
            type="button"
            aria-label={pauseLabel}
            disabled={!canPause}
            onClick={() => void pause()}
            className="flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Pause size={18} />
          </button>
        </TooltipButton>
        <TooltipButton label={finishLabel}>
          <button
            type="button"
            aria-label={finishLabel}
            disabled={!canFinish}
            onClick={() => void finish()}
            className="flex h-8 w-8 items-center justify-center rounded-md bg-green-600 text-white transition-colors hover:bg-green-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Square size={14} fill="currentColor" />
          </button>
        </TooltipButton>
        <TooltipButton label={cancelLabel}>
          <button
            type="button"
            aria-label={cancelLabel}
            disabled={disabled}
            onClick={() => void cancel()}
            className="flex h-8 w-8 items-center justify-center rounded-md border border-red-600 bg-red-600 text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <X size={18} />
          </button>
        </TooltipButton>
      </div>
      {error && (
        <div className="absolute left-1 right-1 top-full mt-1 rounded-md border border-red-500/40 bg-red-50 px-2 py-1 text-[11px] font-semibold text-red-700 shadow">
          {error}
        </div>
      )}
    </div>
  );
}

function TooltipButton({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="group relative shrink-0">
      {children}
      <span className={`${toolbarTooltipClass} bottom-full mb-2`}>{label}</span>
    </div>
  );
}

async function bootstrapRecordingToolbar() {
  let initialLanguage = normalizeLanguage(navigator.language);

  try {
    const settings = await invoke<AppSettings>('get_app_settings');
    initialLanguage = normalizeLanguage(settings.interface_language);
  } catch {
    initialLanguage = normalizeLanguage(navigator.language);
  }

  await setupI18n(initialLanguage);
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <RecordingToolbar />
    </React.StrictMode>,
  );
}

void bootstrapRecordingToolbar();
