import React, { useMemo, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Pause, Play, Square, X } from 'lucide-react';
import './index.css';

type RecordingStatus = 'idle' | 'recording' | 'paused' | 'saving';

function RecordingToolbar() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const sessionId = query.get('session_id') ?? '';
  const [status, setStatus] = useState<RecordingStatus>('idle');
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
        await invoke('resume_gif_recording', { sessionId });
      } else {
        await invoke('start_gif_recording', { sessionId });
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
      await invoke('pause_gif_recording', { sessionId });
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
      await invoke('finish_gif_recording', { sessionId });
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
    await invoke('cancel_gif_recording', { sessionId }).catch(() => undefined);
    await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
    await appWindow.close();
  }

  const disabled = status === 'saving';
  const canStart = !disabled && status !== 'recording';
  const canPause = !disabled && status === 'recording';
  const canFinish = !disabled && status !== 'idle';

  return (
    <div className="flex h-screen items-center justify-center bg-transparent p-1 text-on-surface">
      <div className="flex items-center gap-1.5 rounded-lg border border-outline-variant/30 bg-surface-container-lowest/95 p-1.5 shadow-xl backdrop-blur">
        <button
          type="button"
          title={status === 'paused' ? 'Resume recording' : 'Start recording'}
          aria-label={status === 'paused' ? 'Resume recording' : 'Start recording'}
          disabled={!canStart}
          onClick={() => void startOrResume()}
          className="flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Play size={18} />
        </button>
        <button
          type="button"
          title="Pause recording"
          aria-label="Pause recording"
          disabled={!canPause}
          onClick={() => void pause()}
          className="flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Pause size={18} />
        </button>
        <button
          type="button"
          title="Finish recording"
          aria-label="Finish recording"
          disabled={!canFinish}
          onClick={() => void finish()}
          className="flex h-8 w-8 items-center justify-center rounded-md bg-green-600 text-white transition-colors hover:bg-green-700 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Square size={14} fill="currentColor" />
        </button>
        <button
          type="button"
          title="Cancel"
          aria-label="Cancel"
          disabled={disabled}
          onClick={() => void cancel()}
          className="flex h-8 w-8 items-center justify-center rounded-md border border-red-600 bg-red-600 text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <X size={18} />
        </button>
      </div>
      {error && (
        <div className="absolute left-1 right-1 top-full mt-1 rounded-md border border-red-500/40 bg-red-50 px-2 py-1 text-[11px] font-semibold text-red-700 shadow">
          {error}
        </div>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <RecordingToolbar />
  </React.StrictMode>,
);
