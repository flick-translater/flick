import './capture.css';

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type CaptureContext = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type Selection = {
  x: number;
  y: number;
  width: number;
  height: number;
};

const root = document.querySelector<HTMLDivElement>('#capture-root');

if (!root) {
  throw new Error('Capture root not found');
}

root.innerHTML = `
  <div class="overlay">
    <div id="hint" class="hint"></div>
    <div id="selection" class="selection hidden"></div>
    <div id="crosshair-x" class="crosshair-x"></div>
    <div id="crosshair-y" class="crosshair-y"></div>
  </div>
`;

const hintElement = document.querySelector<HTMLDivElement>('#hint');
const selectionElement = document.querySelector<HTMLDivElement>('#selection');
const crosshairX = document.querySelector<HTMLDivElement>('#crosshair-x');
const crosshairY = document.querySelector<HTMLDivElement>('#crosshair-y');

let context: CaptureContext = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight };
let startPoint: { x: number; y: number } | null = null;
let currentSelection: Selection | null = null;
let isSubmitting = false;
let isReady = false;

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function updateCrosshair(x: number, y: number) {
  crosshairX?.style.setProperty('--crosshair-y', `${y}px`);
  crosshairY?.style.setProperty('--crosshair-x', `${x}px`);
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
  document.body.classList.toggle('capture-ready', ready);
  if (hintElement) {
    hintElement.textContent = ready ? '拖拽选择区域，松开鼠标立即截图，Esc 取消' : '';
  }
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
  setReadyState(false);
  resetSelection();
  context = await invoke<CaptureContext>('get_capture_context');
  await waitForViewport(context);
  setReadyState(true);
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

async function confirmSelection() {
  if (!currentSelection || isSubmitting) {
    return;
  }

  if (currentSelection.width < 2 || currentSelection.height < 2) {
    return;
  }

  isSubmitting = true;

  const scaleX = context.width / window.innerWidth;
  const scaleY = context.height / window.innerHeight;
  const selection = currentSelection;
  resetSelection();
  setReadyState(false);

  try {
    await invoke('complete_capture', {
      selection: {
        x: Math.round(context.x + selection.x * scaleX),
        y: Math.round(context.y + selection.y * scaleY),
        width: Math.round(selection.width * scaleX),
        height: Math.round(selection.height * scaleY)
      }
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

  updateCrosshair(x, y);

  if (!startPoint) {
    return;
  }

  currentSelection = toSelection(x, y);
  drawSelection(currentSelection);
});

window.addEventListener('mousedown', (event) => {
  if (!isReady || event.button !== 0) {
    return;
  }

  startPoint = { x: event.clientX, y: event.clientY };
  currentSelection = null;
  drawSelection(null);
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

void listen('capture-started', () => {
  void loadCaptureContext().catch((error: unknown) => {
    console.error('Failed to refresh capture context', error);
    setReadyState(false);
  });
});
