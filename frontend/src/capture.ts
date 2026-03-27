import './capture.css';

import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

type CaptureContext = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type CursorPosition = {
  x: number;
  y: number;
};

type Selection = {
  x: number;
  y: number;
  width: number;
  height: number;
};

const root = document.querySelector<HTMLDivElement>('#capture-root');
const currentWindow = getCurrentWebviewWindow();

if (!root) {
  throw new Error('Capture root not found');
}

root.innerHTML = `
  <div class="overlay">
    <div id="hint" class="hint"></div>
    <div id="monitor-frame" class="monitor-frame"></div>
    <div id="selection" class="selection hidden"></div>
    <div id="crosshair-x" class="crosshair-x"></div>
    <div id="crosshair-y" class="crosshair-y"></div>
  </div>
`;

const hintElement = document.querySelector<HTMLDivElement>('#hint');
const selectionElement = document.querySelector<HTMLDivElement>('#selection');
const crosshairX = document.querySelector<HTMLDivElement>('#crosshair-x');
const crosshairY = document.querySelector<HTMLDivElement>('#crosshair-y');
const captureChannel = typeof BroadcastChannel !== 'undefined'
  ? new BroadcastChannel('capture-session')
  : null;

let context: CaptureContext = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight };
let startPoint: { x: number; y: number } | null = null;
let currentSelection: Selection | null = null;
let isSubmitting = false;
let isReady = false;
let isLoadingContext = false;
let captureSequence = 0;
let currentCaptureSequence = 0;
let cursorPollTimer: number | null = null;
let hasAutoFocusedFromCursor = false;
let isCaptureSessionActive = false;

let isFocusingWindow = false;

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function updateCrosshair(x: number, y: number) {
  crosshairX?.style.setProperty('--crosshair-y', `${y}px`);
  crosshairY?.style.setProperty('--crosshair-x', `${x}px`);
}

function clearCrosshair() {
  crosshairX?.style.setProperty('--crosshair-y', `-9999px`);
  crosshairY?.style.setProperty('--crosshair-x', `-9999px`);
}

function stopCursorPolling() {
  if (cursorPollTimer !== null) {
    window.clearInterval(cursorPollTimer);
    cursorPollTimer = null;
  }
}

function setHovered(active: boolean, source: string) {
  document.body.classList.toggle('capture-hovered', active);
  void source;
}

function drawSelection(selection: Selection | null) {
  if (!selectionElement) {
    return;
  }

  if (!selection || selection.width < 2 || selection.height < 2) {
    selectionElement.classList.add('hidden');
    return;
  }

  selectionElement.classList.remove('hidden');
  selectionElement.style.left = `${selection.x}px`;
  selectionElement.style.top = `${selection.y}px`;
  selectionElement.style.width = `${selection.width}px`;
  selectionElement.style.height = `${selection.height}px`;
}

function resetSelection() {
  startPoint = null;
  currentSelection = null;
  drawSelection(null);
}

function setReadyState(ready: boolean) {
  isReady = ready;
  hasAutoFocusedFromCursor = false;
  document.body.classList.toggle('capture-ready', ready);
  document.body.classList.remove('capture-hovered');
  if (!ready && !isCaptureSessionActive) {
    stopCursorPolling();
  }
  if (!ready) {
    clearCrosshair();
  }
  if (hintElement) {
    hintElement.textContent = ready ? '拖拽选择区域，松开鼠标立即截图，Esc 取消' : '';
  }
}

function activateCaptureSession(source: string, broadcast: boolean) {
  isCaptureSessionActive = true;
  startCursorPolling();
  if (broadcast) {
    captureChannel?.postMessage({ type: 'capture-started' });
  }
  void source;
}

function deactivateCaptureSession(source: string, broadcast: boolean) {
  isCaptureSessionActive = false;
  if (broadcast) {
    captureChannel?.postMessage({ type: 'capture-ended' });
  }
  void source;
}

function nextFrame() {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve());
  });
}

async function waitForViewport(contextToMatch: CaptureContext) {
  const deadline = performance.now() + 600;

  while (performance.now() < deadline) {
    const widthDiff = Math.abs(window.innerWidth - contextToMatch.width);
    const heightDiff = Math.abs(window.innerHeight - contextToMatch.height);

    if (widthDiff < 1 && heightDiff < 1) {
      return;
    }

    await nextFrame();
  }
}

async function loadCaptureContext() {
  if (isLoadingContext) {
    return;
  }

  isLoadingContext = true;
  const pendingStartPoint = startPoint;
  setReadyState(false);
  if (!pendingStartPoint) {
    resetSelection();
  }
  currentCaptureSequence = captureSequence + 1;
  await nextFrame();
  await nextFrame();
  await invoke<CaptureContext>('refresh_capture_context', { label: currentWindow.label });
  context = await invoke<CaptureContext>('get_capture_context', { label: currentWindow.label });
  await waitForViewport(context);
  captureSequence = currentCaptureSequence;
  setReadyState(true);
  isLoadingContext = false;
  if (pendingStartPoint) {
    startPoint = pendingStartPoint;
    currentSelection = null;
    setHovered(true, 'restore-pending');
    updateCrosshair(pendingStartPoint.x, pendingStartPoint.y);
  }
  startCursorPolling();
}

function ensureCaptureReady(reason: string) {
  if (isReady || isLoadingContext) {
    return;
  }

  void loadCaptureContext().catch((error: unknown) => {
    console.error('Failed to refresh capture context', error);
    isLoadingContext = false;
    setReadyState(false);
  });
  void reason;
}

function isCursorInsideContext(cursor: CursorPosition) {
  return (
    cursor.x >= context.x &&
    cursor.x <= context.x + context.width &&
    cursor.y >= context.y &&
    cursor.y <= context.y + context.height
  );
}

async function pollGlobalCursor() {
  if (!isCaptureSessionActive || startPoint) {
    return;
  }

  try {
    const cursor = await invoke<CursorPosition>('get_global_cursor_position');
    if (!isReady) {
      if (isLoadingContext) {
        return;
      }

      const pendingContext = await invoke<CaptureContext>('get_capture_context', {
        label: currentWindow.label
      }).catch(() => null);
      if (!pendingContext) {
        return;
      }

      const insidePendingContext =
        cursor.x >= pendingContext.x &&
        cursor.x <= pendingContext.x + pendingContext.width &&
        cursor.y >= pendingContext.y &&
        cursor.y <= pendingContext.y + pendingContext.height;

      setHovered(insidePendingContext, 'global-cursor');
      if (!insidePendingContext) {
        clearCrosshair();
        return;
      }

      ensureCaptureReady('global-cursor');
      return;
    }

    const inside = isCursorInsideContext(cursor);
    setHovered(inside, 'global-cursor');
    if (!inside) {
      hasAutoFocusedFromCursor = false;
      clearCrosshair();
      return;
    }

    if (!hasAutoFocusedFromCursor) {
      hasAutoFocusedFromCursor = true;
      void invoke('focus_capture_window', { label: currentWindow.label }).catch(() => {});
    }

    updateCrosshair(cursor.x - context.x, cursor.y - context.y);
  } catch {
  }
}

function startCursorPolling() {
  if (cursorPollTimer !== null) {
    window.clearInterval(cursorPollTimer);
  }

  cursorPollTimer = window.setInterval(() => {
    void pollGlobalCursor();
  }, 16);
}

function toSelection(endX: number, endY: number): Selection | null {
  if (!startPoint) {
    return null;
  }

  const x = Math.min(startPoint.x, endX);
  const y = Math.min(startPoint.y, endY);
  const width = Math.abs(startPoint.x - endX);
  const height = Math.abs(startPoint.y - endY);

  return { x, y, width, height };
}

function beginSelection(x: number, y: number, source: string) {
  startPoint = { x, y };
  currentSelection = null;
  drawSelection(null);
  void source;
}

async function confirmSelection() {
  if (!currentSelection || isSubmitting) {
    return;
  }

  if (currentSelection.width < 2 || currentSelection.height < 2) {
    return;
  }

  isSubmitting = true;

  const widthDiff = Math.abs(context.width - window.innerWidth);
  const heightDiff = Math.abs(context.height - window.innerHeight);
  const scaleX = widthDiff < 2 ? 1 : context.width / window.innerWidth;
  const scaleY = heightDiff < 2 ? 1 : context.height / window.innerHeight;
  const selection = currentSelection;
  const submittedSelection = {
    x: Math.floor(context.x + selection.x * scaleX),
    y: Math.floor(context.y + selection.y * scaleY),
    width: Math.ceil(selection.width * scaleX),
    height: Math.ceil(selection.height * scaleY)
  };
  resetSelection();
  setReadyState(false);

  try {
    await invoke('complete_capture', {
      selection: submittedSelection
    });
  } finally {
    isSubmitting = false;
  }
}

window.addEventListener('mousemove', (event) => {
  if (!isReady) {
    return;
  }

  const x = clamp(event.clientX, 0, window.innerWidth);
  const y = clamp(event.clientY, 0, window.innerHeight);

  if (!startPoint) {
    if ((event.buttons & 1) === 1) {
      beginSelection(x, y, 'mousemove-with-button');
    }
  }

  if (!startPoint) {
    return;
  }

  updateCrosshair(x, y);
  currentSelection = toSelection(x, y);
  drawSelection(currentSelection);
});

window.addEventListener('mousedown', (event) => {
  if (event.button !== 0) {
    return;
  }

  if (!isReady) {
    beginSelection(event.clientX, event.clientY, 'mousedown-pending');
    setHovered(true, 'mousedown-pending');
    ensureCaptureReady('mousedown');
    return;
  }

  beginSelection(event.clientX, event.clientY, 'mousedown');
  setHovered(true, 'mousedown');
});

window.addEventListener('mouseup', (event) => {
  if (!isReady || !startPoint) {
    return;
  }

  currentSelection = toSelection(event.clientX, event.clientY);
  drawSelection(currentSelection);
  startPoint = null;
  void confirmSelection();
});

window.addEventListener('mouseenter', () => {
  if (isReady) {
    setHovered(true, 'mouseenter');
  }
  if (!isFocusingWindow) {
    isFocusingWindow = true;
    void invoke('focus_capture_window', { label: currentWindow.label }).catch(() => {}).finally(() => {
      isFocusingWindow = false;
    });
  }
});

window.addEventListener('mouseleave', () => {
  if (!startPoint) {
    setHovered(false, 'mouseleave');
    clearCrosshair();
  }
});

window.addEventListener('keydown', async (event) => {
  if (event.key === 'Escape') {
    resetSelection();
    setReadyState(false);
    await invoke('cancel_capture');
  }

  if (event.key === 'Enter' && isReady) {
    await confirmSelection();
  }
});

window.addEventListener('dblclick', async () => {
  if (isReady) {
    await confirmSelection();
  }
});

window.addEventListener('contextmenu', async (event) => {
  event.preventDefault();
  resetSelection();
  setReadyState(false);
  await invoke('cancel_capture');
});

void currentWindow.listen('capture-started', () => {
  activateCaptureSession('event:capture-started', true);
  void loadCaptureContext().catch((error: unknown) => {
    console.error('Failed to refresh capture context', error);
    isLoadingContext = false;
    setReadyState(false);
  });
});

void currentWindow.listen('capture-ended', () => {
  deactivateCaptureSession('event:capture-ended', true);
  resetSelection();
  setReadyState(false);
});

window.addEventListener('focus', () => {
  activateCaptureSession('window-focus', true);
  ensureCaptureReady('window-focus');
});

captureChannel?.addEventListener('message', (event) => {
  const type = (event.data as { type?: string } | null)?.type;
  if (type === 'capture-started') {
    activateCaptureSession('broadcast:capture-started', false);
    return;
  }

  if (type === 'capture-ended') {
    deactivateCaptureSession('broadcast:capture-ended', false);
    resetSelection();
    setReadyState(false);
  }
});
