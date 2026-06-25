import { StrictMode, useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { X } from 'lucide-react';
import './index.css';

function decodePath(value: string) {
  const padded = value.replace(/-/g, '+').replace(/_/g, '/')
    + '='.repeat((4 - (value.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function PinnedImage() {
  const appWindow = useMemo(() => getCurrentWindow(), []);
  const initialWindowSizeRef = useRef({ width: 1, height: 1 });
  const imagePath = useMemo(() => {
    const query = new URLSearchParams(window.location.search);
    const encodedPath = query.get('path') ?? '';
    return encodedPath ? decodePath(encodedPath) : '';
  }, []);
  const [dataUrl, setDataUrl] = useState('');
  const [zoom, setZoom] = useState(1);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    initialWindowSizeRef.current = {
      width: Math.max(window.innerWidth, 1),
      height: Math.max(window.innerHeight, 1),
    };
  }, []);

  useEffect(() => {
    if (!imagePath) {
      void appWindow.close();
      return;
    }
    void invoke<string>('read_image_as_data_url', { path: imagePath })
      .then(setDataUrl)
      .catch(() => appWindow.close());
  }, [appWindow, imagePath]);

  async function closeWindow() {
    await appWindow.close();
  }

  async function startDrag() {
    try {
      await appWindow.startDragging();
    } catch {
      // Dragging can fail if the pointer event is no longer current; no recovery is needed.
    }
  }

  async function resizeToZoom(nextZoom: number) {
    const initialSize = initialWindowSizeRef.current;
    await appWindow.setSize(new LogicalSize(
      Math.round(initialSize.width * nextZoom),
      Math.round(initialSize.height * nextZoom),
    ));
  }

  function handleWheel(event: React.WheelEvent<HTMLDivElement>) {
    event.preventDefault();
    setContextMenu(null);
    setZoom((currentZoom) => {
      const direction = event.deltaY < 0 ? 1 : -1;
      const nextZoom = Math.min(4, Math.max(0.2, currentZoom * (direction > 0 ? 1.1 : 0.9)));
      void resizeToZoom(nextZoom);
      return nextZoom;
    });
  }

  return (
    <div
      className="group relative h-screen w-screen overflow-hidden bg-transparent"
      onContextMenu={(event) => {
        event.preventDefault();
        setContextMenu({ x: event.clientX, y: event.clientY });
      }}
      onWheel={handleWheel}
      onPointerDown={(event) => {
        setContextMenu(null);
        if (event.button === 0) {
          void startDrag();
        }
      }}
    >
      {dataUrl && (
        <img
          src={dataUrl}
          alt=""
          draggable={false}
          className="h-full w-full select-none object-contain"
        />
      )}
      <button
        type="button"
        aria-label="Close pinned image"
        onPointerDown={(event) => event.stopPropagation()}
        onClick={() => void closeWindow()}
        className="absolute right-2 top-2 flex h-7 w-7 items-center justify-center rounded-md bg-black/70 text-white opacity-0 shadow-lg ring-1 ring-white/20 transition-opacity hover:bg-black/85 group-hover:opacity-100 focus-visible:opacity-100"
      >
        <X size={18} />
      </button>
      {contextMenu && (
        <div
          className="absolute z-20 min-w-28 overflow-hidden rounded-md border border-white/15 bg-zinc-950/95 py-1 text-sm text-white shadow-2xl"
          style={{
            left: Math.min(contextMenu.x, Math.max(window.innerWidth - 120, 0)),
            top: Math.min(contextMenu.y, Math.max(window.innerHeight - 40, 0)),
          }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            className="flex h-8 w-full items-center px-3 text-left hover:bg-white/10"
            onClick={() => void closeWindow()}
          >
            关闭
          </button>
        </div>
      )}
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PinnedImage />
  </StrictMode>,
);
