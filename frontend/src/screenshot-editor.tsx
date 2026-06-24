import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import {
  LogicalPosition,
  LogicalSize,
  currentMonitor,
  getCurrentWindow,
} from '@tauri-apps/api/window';
import './index.css';
import { ScreenshotEditorCanvas } from './components/screenshot-editor/Canvas';
import { emojiChoices } from './components/screenshot-editor/emoji-data';
import {
  GifRecordingToolbar,
  LongScreenshotToolbar,
  ScreenshotEditorToolbar,
} from './components/screenshot-editor/Toolbar';
import { normalizeLanguage, setupI18n } from './i18n/config';
import type {
  Annotation,
  AnnotationDragState,
  Point,
  Rect,
  ResizeHandle,
  TextDraft,
  Tool,
} from './components/screenshot-editor/types';
import type { AppSettings } from './types';

const defaultEditorColor = '#ef4444';
const imageSizeFallback = { width: 1, height: 1 };
const emojiPageSize = 48;
const emojiDefaultSize = 64;
const emojiMinSize = 18;
const emojiHandleSize = 8;
const selectionMinSize = 18;
const defaultLongScreenshotThumbnailWidth = 300;
const longEditToolbarMinWidth = 560;
const longEditToolbarHeight = 48;
const longEditPanelClearance = 288;
const longEditMinImageHeight = 120;
const longEditMinWindowHeight = longEditToolbarHeight
  + longEditPanelClearance * 2
  + longEditMinImageHeight;

type EditorMode = 'edit' | 'long-capture' | 'recording';
type GifRecordingStatus = 'idle' | 'recording' | 'paused' | 'saving';

function waitForNextPaint() {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve());
    });
  });
}

type PreviewSegment = {
  id: number;
  dataUrl: string;
  rows: number;
};

type LongScreenshotState = {
  active: boolean;
  scrollOffset: number;
  minOffset: number;
  maxOffset: number;
  frameHeight: number;
  totalHeight: number;
  currentFrameDataUrl: string;
  previewDataUrl: string;
  previewSegments: PreviewSegment[];
};

type LongCaptureUpdate = {
  current_frame_data_url: string;
  preview_data_url: string;
  preview_append_data_url?: string;
  preview_append_rows?: number;
  preview_prepend_data_url?: string;
  preview_prepend_rows?: number;
  width: number;
  frame_height: number;
  total_height: number;
  scroll_offset: number;
  min_offset: number;
  max_offset: number;
};

function editorLog(step: string) {
  if (!step.includes('gif recording') && !step.includes('gif-recording')) {
    return;
  }
  console.info(`[gif-recording/frontend] ${step}`);
  void invoke('capture_editor_frontend_log', { message: step }).catch(() => undefined);
}

function ScreenshotEditor() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const appWindow = useMemo(() => getCurrentWindow(), []);
  const initialColor = useMemo(() => {
    const rawColor = query.get('color') ?? '';
    const normalized = rawColor.startsWith('#') ? rawColor : `#${rawColor}`;
    return isHexColor(normalized) ? normalized.toLowerCase() : defaultEditorColor;
  }, [query]);
  const sessionId = query.get('session_id') ?? '';
  const isPreload = query.get('preload') === '1';
  const isLongEditLaunch = query.get('long_edit') === '1';
  const isLinux = useMemo(() => /Linux/i.test(navigator.platform), []);
  const isWindows = useMemo(() => /Win/i.test(navigator.platform), []);
  const windowLabel = appWindow.label;
  const baseCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const draftCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  const isFinishedRef = useRef(false);
  const isClosingRef = useRef(false);
  const readyNotifiedRef = useRef(false);
  const longUpdateSignatureRef = useRef('');
  const longScrollActiveRef = useRef(false);
  const longScrollInFlightRef = useRef(false);
  const previewSegmentIdRef = useRef(0);
  const annotationsRef = useRef<Annotation[]>([]);
  const draftRef = useRef<Annotation | null>(null);
  const dragStartRef = useRef<Point | null>(null);
  const annotationDragRef = useRef<AnnotationDragState | null>(null);
  const [imageLoaded, setImageLoaded] = useState(false);
  const [editorVisible, setEditorVisible] = useState(true);
  const [imageSize, setImageSize] = useState(imageSizeFallback);
  const [screenshotDataUrl, setScreenshotDataUrl] = useState('');
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [undoStack, setUndoStack] = useState<Annotation[][]>([]);
  const [redoStack, setRedoStack] = useState<Annotation[][]>([]);
  const [tool, setTool] = useState<Tool | null>(null);
  const [color, setColor] = useState(initialColor);
  const [colorPickerOpen, setColorPickerOpen] = useState(false);
  const [hexInput, setHexInput] = useState(initialColor);
  const [lineWidth, setLineWidth] = useState(4);
  const [fontSize, setFontSize] = useState(28);
  const [mosaicSize, setMosaicSize] = useState(12);
  const [mosaicWidth, setMosaicWidth] = useState(28);
  const [isDragging, setIsDragging] = useState(false);
  const [textDraft, setTextDraft] = useState<TextDraft | null>(null);
  const [selectedAnnotationIndex, setSelectedAnnotationIndex] = useState<number | null>(null);
  const [emojiPickerOpen, setEmojiPickerOpen] = useState(false);
  const [emojiPage, setEmojiPage] = useState(0);
  const [canvasCursor, setCanvasCursor] = useState('default');
  const [error, setError] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [editorMode, setEditorMode] = useState<EditorMode>('edit');
  const [gifRecordingStatus, setGifRecordingStatus] = useState<GifRecordingStatus>('idle');
  const [isLongEditWindow, setIsLongEditWindow] = useState(false);
  const [longEditDisplaySize, setLongEditDisplaySize] = useState(imageSizeFallback);
  const [longScreenshot, setLongScreenshot] = useState<LongScreenshotState>({
    active: false,
    scrollOffset: 0,
    minOffset: 0,
    maxOffset: 0,
    frameHeight: 0,
    totalHeight: 0,
    currentFrameDataUrl: '',
    previewDataUrl: '',
    previewSegments: [],
  });
  const displaySize = {
    width: Number(query.get('display_width')) || imageSize.width,
    height: Number(query.get('display_height')) || imageSize.height,
  };
  const selectionOffset = {
    left: Number(query.get('selection_left')) || 0,
    top: Number(query.get('selection_top')) || 0,
  };
  const toolbarOffset = {
    left: Number(query.get('toolbar_left')) || 8,
    top: Number(query.get('toolbar_top')) || displaySize.height + 8,
  };
  const longScreenshotThumbnailWidth =
    Number(query.get('thumbnail_width')) || defaultLongScreenshotThumbnailWidth;
  const longScreenshotThumbnailHeightFromQuery = Number(query.get('thumbnail_height')) || 0;
  const longScreenshotThumbnailLeftFromQuery = Number(query.get('thumbnail_left'));
  const longScreenshotThumbnailTopFromQuery = Number(query.get('thumbnail_top'));
  const longScreenshotThumbnailRegionTopFromQuery = Number(query.get('thumbnail_region_top'));
  const popupPlacement = query.get('popup_placement') === 'up' ? 'up' : 'down';
  const popupPositionClass = popupPlacement === 'up' ? 'bottom-[calc(100%+8px)]' : 'top-[calc(100%+8px)]';
  const [toolbarPosition, setToolbarPosition] = useState(toolbarOffset);
  const emojiPageCount = Math.max(1, Math.ceil(emojiChoices.length / emojiPageSize));
  const visibleEmojiChoices = emojiChoices.slice(emojiPage * emojiPageSize, (emojiPage + 1) * emojiPageSize);

  useEffect(() => {
    annotationsRef.current = annotations;
    renderCommitted();
    if (
      selectedAnnotationIndex !== null
      && !isSelectableAnnotation(annotations[selectedAnnotationIndex])
    ) {
      setSelectedAnnotationIndex(null);
    }
  }, [annotations]);

  useEffect(() => {
    if (isPreload) {
      return;
    }
    const updateToolbarPosition = () => {
      const toolbar = toolbarRef.current;
      if (!toolbar) {
        return;
      }
      const bounds = toolbar.getBoundingClientRect();
      const left = Math.min(
        Math.max(toolbarOffset.left, 8),
        Math.max(window.innerWidth - bounds.width - 8, 8),
      );
      const top = Math.min(
        Math.max(toolbarOffset.top, 8),
        Math.max(window.innerHeight - bounds.height - 8, 8),
      );
      setToolbarPosition({ left, top });
    };
    requestAnimationFrame(updateToolbarPosition);
    window.addEventListener('resize', updateToolbarPosition);
    return () => window.removeEventListener('resize', updateToolbarPosition);
  }, [isPreload, toolbarOffset.left, toolbarOffset.top]);

  useEffect(() => {
    if (isPreload) {
      editorLog('preload window ready');
      return;
    }
    if (!sessionId) {
      setError('Missing editor session.');
      return;
    }

    document.body.tabIndex = -1;
    document.body.focus();
    window.focus();
    editorLog(
      `window load: label=${windowLabel} session=${sessionId} preload=${isPreload} long_edit=${isLongEditLaunch} href=${window.location.href}`,
    );

    const imageCommand = isLongEditLaunch ? 'get_long_capture_image' : 'get_pending_capture_image';
    editorLog(`frontend load: ${imageCommand} start`);
    void invoke<string>(imageCommand, { sessionId })
      .then((dataUrl) => {
        editorLog(`frontend load: data url received length=${dataUrl.length}`);
        setScreenshotDataUrl(dataUrl);
        const image = new Image();
        image.onload = () => {
          editorLog(`frontend load: image decoded natural=${image.naturalWidth}x${image.naturalHeight}`);
          imageRef.current = image;
          setImageSize({ width: image.naturalWidth, height: image.naturalHeight });
          if (isLongEditLaunch) {
            setLongEditDisplaySize({
              width: image.naturalWidth,
              height: image.naturalHeight,
            });
          }
          for (const canvas of [baseCanvasRef.current, draftCanvasRef.current]) {
            if (canvas) {
              canvas.width = image.naturalWidth;
              canvas.height = image.naturalHeight;
            }
          }
          setImageLoaded(true);
          editorLog('frontend load: canvas sized');
          if (isLongEditLaunch) {
            void invoke('prepare_long_capture_edit', { sessionId });
            setIsLongEditWindow(true);
            setEditorMode('edit');
            void resizeWindowForLongEdit(image.naturalWidth, image.naturalHeight, true);
          }
          requestAnimationFrame(() => {
            renderCommitted();
            requestAnimationFrame(() => {
              if (readyNotifiedRef.current) {
                return;
              }
              readyNotifiedRef.current = true;
              editorLog('frontend load: first frame rendered; notify backend ready');
              void invoke('capture_editor_ready', { sessionId })
                .catch((readyError: unknown) => {
                  editorLog(`frontend load: capture_editor_ready failed ${String(readyError)}`);
                })
                .finally(() => {
                  setEditorVisible(true);
                });
            });
          });
          editorLog('frontend load: initial render scheduled');
        };
        image.onerror = () => {
          if (isClosingRef.current || isFinishedRef.current) {
            return;
          }
          editorLog('frontend load: image decode failed');
          setError('Failed to load screenshot.');
        };
        image.src = dataUrl;
      })
      .catch((loadError: unknown) => {
        if (isClosingRef.current || isFinishedRef.current) {
          return;
        }
        editorLog(`frontend load: failed ${String(loadError)}`);
        setError(String(loadError));
      });
  }, [isPreload, isLongEditLaunch, sessionId, windowLabel]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.type === 'keyup' && event.key !== 'Escape') {
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        if (textDraft) {
          setTextDraft(null);
          return;
        }
        void handleCancel();
        return;
      }

      const modifier = event.ctrlKey || event.metaKey;
      if (!modifier) {
        return;
      }

      const key = event.key.toLowerCase();
      if (key === 'z') {
        if (textDraft) {
          return;
        }
        event.preventDefault();
        if (event.shiftKey) {
          redo();
        } else {
          undo();
        }
      } else if (key === 'y') {
        event.preventDefault();
        redo();
      }
    };

    document.addEventListener('keydown', handleKeyDown, true);
    document.addEventListener('keyup', handleKeyDown, true);
    window.addEventListener('keydown', handleKeyDown, true);
    window.addEventListener('keyup', handleKeyDown, true);
    return () => {
      document.removeEventListener('keydown', handleKeyDown, true);
      document.removeEventListener('keyup', handleKeyDown, true);
      window.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('keyup', handleKeyDown, true);
    };
  });

  useEffect(() => {
    if (!textDraft) {
      return;
    }
    requestAnimationFrame(() => {
      textAreaRef.current?.focus();
    });
  }, [textDraft]);

  // The backend watches real scroll-wheel activity (the selection region is transparent so
  // the wheel scrolls the real page) and pushes a fresh stitched preview after each scroll
  // settles. Apply those updates here.
  useEffect(() => {
    if (isPreload) {
      return;
    }
    editorLog('long update listener: registering window');
    const windowUnlisten = appWindow.listen<LongCaptureUpdate>('long-capture-update', (event) => {
      editorLog('long update listener: window event received');
      applyLongCaptureUpdate(event.payload);
    });
    return () => {
      void windowUnlisten.then((dispose) => dispose());
    };
  }, [appWindow, isPreload]);

  useEffect(() => {
    const unlistenPromise = appWindow.onCloseRequested(async (event) => {
      editorLog(
        `window close requested: label=${windowLabel} session=${sessionId || '<missing>'} finished=${isFinishedRef.current} closing=${isClosingRef.current} mode=${editorMode} long_edit=${isLongEditLaunch}`,
      );
      if (isFinishedRef.current || !sessionId) {
        editorLog(`window close allowed: label=${windowLabel}`);
        return;
      }
      event.preventDefault();
      isClosingRef.current = true;
      isFinishedRef.current = true;
      stopLongScroll();
      if (editorMode === 'long-capture') {
        editorLog(`window close prevented: invoke cancel_long_capture label=${windowLabel}`);
        await invoke('cancel_long_capture', { sessionId }).catch(() => undefined);
      }
      if (editorMode === 'recording') {
        editorLog(`window close prevented: invoke cancel_gif_recording label=${windowLabel}`);
        await invoke('cancel_gif_recording', { sessionId }).catch(() => undefined);
        editorLog(`gif recording window close: invoke cancel_gif_recording complete label=${windowLabel}`);
        await invoke('close_gif_recording_toolbar_window', { sessionId }).catch(() => undefined);
      }
      editorLog(`window close prevented: invoke cancel_capture_edit label=${windowLabel}`);
      await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
      editorLog(`window close prevented: closing label=${windowLabel}`);
      await appWindow.close();
    });

    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [appWindow, editorMode, isLongEditLaunch, sessionId, windowLabel]);

  useEffect(() => {
    return () => {
      stopLongScroll();
    };
  }, []);

  const commitAnnotations = useCallback((next: Annotation[]) => {
    setAnnotations((current) => {
      setUndoStack((stack) => [...stack, current]);
      setRedoStack([]);
      return next;
    });
  }, []);

  const undo = useCallback(() => {
    setUndoStack((current) => {
      if (current.length === 0) {
        return current;
      }
      const restored = current[current.length - 1];
      setAnnotations((items) => {
        setRedoStack((stack) => [...stack, items]);
        return restored;
      });
      return current.slice(0, -1);
    });
  }, []);

  const redo = useCallback(() => {
    setRedoStack((current) => {
      if (current.length === 0) {
        return current;
      }
      const restored = current[current.length - 1];
      setAnnotations((items) => {
        setUndoStack((stack) => [...stack, items]);
        return restored;
      });
      return current.slice(0, -1);
    });
  }, []);

  const addAnnotation = useCallback((annotation: Annotation) => {
    const nextIndex = annotationsRef.current.length;
    commitAnnotations([...annotationsRef.current, annotation]);
    setSelectedAnnotationIndex(isSelectableAnnotation(annotation) ? nextIndex : null);
  }, [commitAnnotations]);

  const selectedAnnotation = selectedAnnotationIndex === null ? null : annotations[selectedAnnotationIndex];
  const selectedAnnotationRect = selectedAnnotation && isSelectableAnnotation(selectedAnnotation)
    ? getAnnotationBounds(selectedAnnotation)
    : null;
  const selectedControlRect = selectedAnnotationRect ? expandRectToMinSize(selectedAnnotationRect, selectionMinSize) : null;

  function renderCommitted() {
    const canvas = baseCanvasRef.current;
    const image = imageRef.current;
    if (!canvas || !image) {
      editorLog(`renderCommitted: skipped canvas=${Boolean(canvas)} image=${Boolean(image)}`);
      return;
    }

    const context = canvas.getContext('2d');
    if (!context) {
      editorLog('renderCommitted: skipped missing 2d context');
      return;
    }

    const bounds = canvas.getBoundingClientRect();
    editorLog(
      `renderCommitted: draw start canvas=${canvas.width}x${canvas.height} image=${image.naturalWidth}x${image.naturalHeight} rect=${Math.round(bounds.left)},${Math.round(bounds.top)},${Math.round(bounds.width)}x${Math.round(bounds.height)} selection=${selectionOffset.left},${selectionOffset.top},${displaySize.width}x${displaySize.height}`,
    );
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    for (const annotation of annotationsRef.current) {
      drawAnnotation(context, annotation);
    }
    editorLog(`renderCommitted: draw complete ${canvasSampleSummary(context, canvas)}`);
  }

  function renderDraft(annotation: Annotation | null) {
    const draftCanvas = draftCanvasRef.current;
    if (!draftCanvas) {
      return;
    }
    const context = draftCanvas.getContext('2d');
    if (!context) {
      return;
    }
    context.clearRect(0, 0, draftCanvas.width, draftCanvas.height);
    if (!annotation) {
      return;
    }

    if (annotation.kind === 'mosaic') {
      const baseCanvas = baseCanvasRef.current;
      if (baseCanvas) {
        context.drawImage(baseCanvas, 0, 0);
      }
    }
    drawAnnotation(context, annotation);
  }

  function getCanvasPoint(event: React.PointerEvent<HTMLCanvasElement>): Point {
    const canvas = draftCanvasRef.current;
    if (!canvas) {
      return { x: 0, y: 0 };
    }
    const bounds = canvas.getBoundingClientRect();
    return {
      x: ((event.clientX - bounds.left) / bounds.width) * canvas.width,
      y: ((event.clientY - bounds.top) / bounds.height) * canvas.height,
    };
  }

  function getCssPoint(event: React.PointerEvent<HTMLCanvasElement>): Point {
    const canvas = draftCanvasRef.current;
    if (!canvas) {
      return { x: 0, y: 0 };
    }
    const bounds = canvas.getBoundingClientRect();
    return {
      x: event.clientX - bounds.left,
      y: event.clientY - bounds.top,
    };
  }

  function getCanvasScale() {
    const canvas = draftCanvasRef.current;
    if (!canvas) {
      return { x: 1, y: 1 };
    }
    const bounds = canvas.getBoundingClientRect();
    return {
      x: bounds.width / Math.max(canvas.width, 1),
      y: bounds.height / Math.max(canvas.height, 1),
    };
  }

  function canvasRectToCss(rect: Rect): Rect {
    const scale = getCanvasScale();
    return {
      x: rect.x * scale.x,
      y: rect.y * scale.y,
      width: rect.width * scale.x,
      height: rect.height * scale.y,
    };
  }

  function buildShape(from: Point, to: Point, constrain: boolean): Annotation {
    const adjustedTo = constrain ? constrainPoint(from, to) : to;
    if (tool === 'line') {
      return { kind: 'line', from, to: adjustedTo, color, width: lineWidth };
    }
    if (tool === 'arrow') {
      return { kind: 'arrow', from, to: adjustedTo, color, width: lineWidth };
    }
    if (tool === 'ellipse') {
      return { kind: 'ellipse', rect: rectFromPoints(from, adjustedTo, constrain), color, width: lineWidth };
    }
    return { kind: 'rect', rect: rectFromPoints(from, adjustedTo, constrain), color, width: lineWidth };
  }

  function handleToolClick(nextTool: Tool) {
    setColorPickerOpen(false);
    setTool((currentTool) => {
      const nextActiveTool = currentTool === nextTool ? null : nextTool;
      setEmojiPickerOpen(nextActiveTool === 'emoji');
      if (nextActiveTool === 'emoji') {
        setEmojiPage(0);
      }
      return nextActiveTool;
    });
  }

  function handleEmojiChoice(emoji: string) {
    const size = Math.min(emojiDefaultSize, Math.max(emojiMinSize, Math.min(imageSize.width, imageSize.height) * 0.25));
    const annotation: Annotation = {
      kind: 'emoji',
      emoji,
      rect: {
        x: Math.max(0, imageSize.width / 2 - size / 2),
        y: Math.max(0, imageSize.height / 2 - size / 2),
        width: size,
        height: size,
      },
    };
    const nextAnnotations = [...annotationsRef.current, annotation];
    commitAnnotations(nextAnnotations);
    setSelectedAnnotationIndex(nextAnnotations.length - 1);
    setEmojiPickerOpen(false);
  }

  function updateAnnotationAt(index: number, annotation: Annotation) {
    const nextAnnotations = annotationsRef.current.map((item, itemIndex) => (
      itemIndex === index ? annotation : item
    ));
    annotationsRef.current = nextAnnotations;
    setAnnotations(nextAnnotations);
  }

  function finishAnnotationDrag() {
    const drag = annotationDragRef.current;
    if (!drag) {
      return;
    }
    annotationDragRef.current = null;
    setIsDragging(false);
    const current = annotationsRef.current[drag.annotationIndex];
    const currentRect = current && isSelectableAnnotation(current) ? getAnnotationBounds(current) : null;
    if (currentRect && !sameRect(currentRect, drag.initialRect)) {
      setUndoStack((stack) => [...stack, drag.initialAnnotations]);
      setRedoStack([]);
    }
  }

  function handlePointerDown(event: React.PointerEvent<HTMLCanvasElement>) {
    if (!imageRef.current) {
      return;
    }
    setColorPickerOpen(false);
    setEmojiPickerOpen(false);
    const point = getCanvasPoint(event);
    const handleHit = selectedControlRect ? hitResizeHandle(point, selectedControlRect, getCanvasScale()) : null;
    if (selectedAnnotationIndex !== null && selectedAnnotationRect && handleHit) {
      event.currentTarget.setPointerCapture(event.pointerId);
      annotationDragRef.current = {
        annotationIndex: selectedAnnotationIndex,
        initialAnnotations: annotationsRef.current,
        initialRect: selectedAnnotationRect,
        startPoint: point,
        mode: 'resize',
        handle: handleHit,
      };
      setIsDragging(true);
      setCanvasCursor(getResizeCursor(handleHit));
      return;
    }

    const annotationHitIndex = hitSelectableAnnotation(point, annotationsRef.current);
    if (annotationHitIndex !== null) {
      const annotation = annotationsRef.current[annotationHitIndex];
      if (!isSelectableAnnotation(annotation)) {
        return;
      }
      event.currentTarget.setPointerCapture(event.pointerId);
      setSelectedAnnotationIndex(annotationHitIndex);
      annotationDragRef.current = {
        annotationIndex: annotationHitIndex,
        initialAnnotations: annotationsRef.current,
        initialRect: getAnnotationBounds(annotation),
        startPoint: point,
        mode: 'move',
      };
      setIsDragging(true);
      setCanvasCursor('grabbing');
      return;
    }

    setSelectedAnnotationIndex(null);
    if (!tool) {
      return;
    }

    if (tool === 'text') {
      setTextDraft({ position: point, cssPosition: getCssPoint(event), value: '' });
      return;
    }
    if (tool === 'emoji') {
      return;
    }

    event.currentTarget.setPointerCapture(event.pointerId);
    dragStartRef.current = point;
    setIsDragging(true);
    const initial: Annotation = tool === 'pen'
      ? { kind: 'pen', points: [point], color, width: lineWidth }
      : tool === 'mosaic'
        ? { kind: 'mosaic', points: [point], width: mosaicWidth, blockSize: mosaicSize }
        : buildShape(point, point, event.shiftKey);
    draftRef.current = initial;
    renderDraft(initial);
  }

  function handlePointerMove(event: React.PointerEvent<HTMLCanvasElement>) {
    if (annotationDragRef.current) {
      const drag = annotationDragRef.current;
      const point = getCanvasPoint(event);
      const initial = drag.initialAnnotations[drag.annotationIndex];
      if (!isSelectableAnnotation(initial)) {
        return;
      }
      const delta = { x: point.x - drag.startPoint.x, y: point.y - drag.startPoint.y };
      const updated = drag.mode === 'move'
        ? moveAnnotation(initial, delta, imageSize)
        : resizeAnnotation(initial, drag.initialRect, delta, drag.handle ?? 'se', imageSize);
      updateAnnotationAt(drag.annotationIndex, updated);
      return;
    }

    if (!isDragging || !draftRef.current || !dragStartRef.current) {
      setCanvasCursor(getCanvasCursor(getCanvasPoint(event)));
      return;
    }

    const point = getCanvasPoint(event);
    const draft = draftRef.current;
    if (draft.kind === 'pen') {
      draftRef.current = { ...draft, points: [...draft.points, point] };
    } else if (draft.kind === 'mosaic') {
      draftRef.current = { ...draft, points: [...draft.points, point] };
    } else {
      draftRef.current = buildShape(dragStartRef.current, point, event.shiftKey);
    }
    renderDraft(draftRef.current);
  }

  function handlePointerUp(event: React.PointerEvent<HTMLCanvasElement>) {
    if (annotationDragRef.current) {
      event.currentTarget.releasePointerCapture(event.pointerId);
      finishAnnotationDrag();
      setCanvasCursor(getCanvasCursor(getCanvasPoint(event)));
      return;
    }

    if (!isDragging || !draftRef.current) {
      return;
    }
    event.currentTarget.releasePointerCapture(event.pointerId);
    const draft = draftRef.current;
    const shouldKeep = draft.kind === 'pen' || draft.kind === 'mosaic'
      ? draft.points.length > 1
      : true;
    if (shouldKeep) {
      addAnnotation(draft);
    }
    draftRef.current = null;
    dragStartRef.current = null;
    setIsDragging(false);
    setCanvasCursor(getCanvasCursor(getCanvasPoint(event)));
    renderDraft(null);
  }

  function getCanvasCursor(point: Point) {
    if (selectedControlRect) {
      const handle = hitResizeHandle(point, selectedControlRect, getCanvasScale());
      if (handle) {
        return getResizeCursor(handle);
      }
    }

    if (hitSelectableAnnotation(point, annotationsRef.current) !== null) {
      return 'grab';
    }

    return tool ? 'crosshair' : 'default';
  }

  function commitTextDraft() {
    if (!textDraft?.value.trim()) {
      setTextDraft(null);
      return;
    }
    addAnnotation({
      kind: 'text',
      position: textDraft.position,
      text: textDraft.value.trim(),
      color,
      fontSize,
    });
    setTextDraft(null);
  }

  function handleColorChange(nextColor: string) {
    if (!isHexColor(nextColor)) {
      return;
    }
    const normalized = nextColor.toLowerCase();
    setColor(normalized);
    setHexInput(normalized);
    void invoke<AppSettings>('update_screenshot_editor_color', { color: normalized })
      .then((settings) => {
        if (isHexColor(settings.screenshot_editor_color)) {
          setColor(settings.screenshot_editor_color);
          setHexInput(settings.screenshot_editor_color);
        }
      })
      .catch((saveError: unknown) => {
        editorLog(`color save failed ${String(saveError)}`);
      });
  }

  function handleColorSquarePointer(event: React.PointerEvent<HTMLDivElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = Math.min(Math.max(event.clientX - bounds.left, 0), bounds.width);
    const y = Math.min(Math.max(event.clientY - bounds.top, 0), bounds.height);
    const hsv = hexToHsv(color);
    handleColorChange(hsvToHex({
      h: hsv.h,
      s: x / bounds.width,
      v: 1 - y / bounds.height,
    }));
  }

  function handleHexInputCommit() {
    if (isHexColor(hexInput)) {
      handleColorChange(hexInput);
    } else {
      setHexInput(color);
    }
  }

  function buildEditedPngBase64() {
    const image = imageRef.current;
    if (!image) {
      throw new Error('Screenshot image is not loaded.');
    }
    const exportCanvas = document.createElement('canvas');
    exportCanvas.width = image.naturalWidth;
    exportCanvas.height = image.naturalHeight;
    const context = exportCanvas.getContext('2d');
    if (!context) {
      throw new Error('Failed to create export canvas.');
    }
    context.drawImage(image, 0, 0);
    for (const annotation of annotationsRef.current) {
      drawAnnotation(context, annotation);
    }
    return exportCanvas.toDataURL('image/png');
  }

  function applyLongCaptureUpdate(update: LongCaptureUpdate) {
    const previewAppendDataUrl = update.preview_append_data_url ?? '';
    const previewAppendRows = update.preview_append_rows ?? 0;
    const previewPrependDataUrl = update.preview_prepend_data_url ?? '';
    const previewPrependRows = update.preview_prepend_rows ?? 0;
    const signature = [
      update.total_height,
      update.scroll_offset,
      update.preview_data_url,
      previewAppendDataUrl,
      previewAppendRows,
      previewPrependDataUrl,
      previewPrependRows,
      update.current_frame_data_url,
    ].join(':');
    if (longUpdateSignatureRef.current === signature) {
      return;
    }
    longUpdateSignatureRef.current = signature;
    editorLog(
      `long update: frame=${update.width}x${update.frame_height} total=${update.total_height} offset=${update.scroll_offset} min=${update.min_offset} max=${update.max_offset} preview_len=${update.preview_data_url.length} preview_append_len=${previewAppendDataUrl.length} preview_append_rows=${previewAppendRows} preview_prepend_len=${previewPrependDataUrl.length} preview_prepend_rows=${previewPrependRows} current_len=${update.current_frame_data_url.length}`,
    );
    setLongScreenshot((previous) => {
      let previewSegments = previous.previewSegments;
      let previewDataUrl = previous.previewDataUrl;
      if (update.preview_data_url) {
        previewDataUrl = update.preview_data_url;
        previewSegments = [{
          id: ++previewSegmentIdRef.current,
          dataUrl: update.preview_data_url,
          rows: update.total_height,
        }];
      } else if (previewAppendDataUrl && previewAppendRows > 0) {
        previewSegments = [
          ...previous.previewSegments,
          {
            id: ++previewSegmentIdRef.current,
            dataUrl: previewAppendDataUrl,
            rows: previewAppendRows,
          },
        ];
      } else if (previewPrependDataUrl && previewPrependRows > 0) {
        // Scrolling up adds rows at the top — insert at the head so the preview grows upward
        // incrementally instead of re-sending the whole image.
        previewSegments = [
          {
            id: ++previewSegmentIdRef.current,
            dataUrl: previewPrependDataUrl,
            rows: previewPrependRows,
          },
          ...previous.previewSegments,
        ];
      }
      return {
        active: true,
        scrollOffset: update.scroll_offset,
        minOffset: update.min_offset,
        maxOffset: update.max_offset,
        frameHeight: update.frame_height,
        totalHeight: update.total_height,
        currentFrameDataUrl: update.current_frame_data_url,
        previewDataUrl,
        previewSegments,
      };
    });
  }

  async function handleLongCaptureStart() {
    editorLog(
      `long start click: session=${sessionId || '<missing>'} isSaving=${isSaving} imageLoaded=${imageLoaded} mode=${editorMode} selection=${selectionOffset.left},${selectionOffset.top},${displaySize.width}x${displaySize.height}`,
    );
    if (!sessionId || isSaving) {
      editorLog('long start ignored: missing session or saving');
      return;
    }
    if (isLinux) {
      editorLog('long start ignored: unsupported on linux');
      return;
    }
    setTool(null);
    setColorPickerOpen(false);
    setEmojiPickerOpen(false);
    setSelectedAnnotationIndex(null);
    setTextDraft(null);
    setIsSaving(true);
    setError('');
    setIsLongEditWindow(false);
    setLongEditDisplaySize(imageSizeFallback);
    setEditorMode('long-capture');
    try {
      await waitForNextPaint();
      editorLog('long start invoke start_long_capture');
      const update = await invoke<LongCaptureUpdate>('start_long_capture', { sessionId });
      editorLog('long start invoke complete');
      applyLongCaptureUpdate(update);
    } catch (startError) {
      editorLog(`long start failed: ${String(startError)}`);
      setEditorMode('edit');
      setError(String(startError));
    } finally {
      setIsSaving(false);
    }
  }

  async function handleGifRecordingModeStart() {
    editorLog(
      `gif recording mode click: session=${sessionId || '<missing>'} isSaving=${isSaving} imageLoaded=${imageLoaded} mode=${editorMode} selection=${selectionOffset.left},${selectionOffset.top},${displaySize.width}x${displaySize.height} toolbar=${toolbarPosition.left},${toolbarPosition.top}`,
    );
    if (!sessionId || isSaving) {
      editorLog('gif recording mode click ignored: missing session or saving');
      return;
    }
    setTool(null);
    setColorPickerOpen(false);
    setEmojiPickerOpen(false);
    setSelectedAnnotationIndex(null);
    setTextDraft(null);
    setError('');
    setGifRecordingStatus('idle');
    editorLog('gif recording mode: set editorMode=recording status=idle');
    setEditorMode('recording');
    try {
      editorLog('gif recording mode: wait for recording frame paint start');
      await waitForNextPaint();
      editorLog('gif recording mode: wait for recording frame paint complete');
      if (isWindows) {
        editorLog('gif recording mode: invoke set_gif_recording_window_shape recording=true start');
        await invoke('set_gif_recording_window_shape', { sessionId, recording: true });
        editorLog('gif recording mode: invoke set_gif_recording_window_shape recording=true complete');
      }
      editorLog('gif recording mode: invoke prepare_gif_recording_mode start');
      await invoke('prepare_gif_recording_mode', { sessionId });
      editorLog('gif recording mode: invoke prepare_gif_recording_mode complete');
    } catch (recordingModeError) {
      editorLog(`gif recording mode failed ${String(recordingModeError)}`);
      setEditorMode('edit');
      setError(String(recordingModeError));
    }
  }

  async function handleGifRecordingStart() {
    editorLog(
      `gif recording start click: session=${sessionId || '<missing>'} status=${gifRecordingStatus} mode=${editorMode}`,
    );
    if (!sessionId || gifRecordingStatus === 'recording' || gifRecordingStatus === 'saving') {
      editorLog('gif recording start ignored: missing session or already recording/saving');
      return;
    }
    setError('');
    try {
      editorLog('gif recording start: invoke set_gif_recording_window_shape recording=true start');
      await invoke('set_gif_recording_window_shape', { sessionId, recording: true });
      editorLog('gif recording start: invoke set_gif_recording_window_shape recording=true complete');
      if (gifRecordingStatus === 'paused') {
        editorLog('gif recording start: invoke resume_gif_recording start');
        await invoke('resume_gif_recording', { sessionId });
        editorLog('gif recording start: invoke resume_gif_recording complete');
      } else {
        editorLog('gif recording start: invoke start_gif_recording start');
        await invoke('start_gif_recording', { sessionId });
        editorLog('gif recording start: invoke start_gif_recording complete');
      }
      setGifRecordingStatus('recording');
      editorLog('gif recording start: status=recording');
    } catch (recordingError) {
      editorLog(`gif recording start failed: ${String(recordingError)}`);
      setError(String(recordingError));
      setGifRecordingStatus('idle');
    }
  }

  async function handleGifRecordingPause() {
    editorLog(
      `gif recording pause click: session=${sessionId || '<missing>'} status=${gifRecordingStatus}`,
    );
    if (!sessionId || gifRecordingStatus !== 'recording') {
      editorLog('gif recording pause ignored: missing session or status is not recording');
      return;
    }
    setError('');
    try {
      editorLog('gif recording pause: invoke pause_gif_recording start');
      await invoke('pause_gif_recording', { sessionId });
      editorLog('gif recording pause: invoke pause_gif_recording complete');
      setGifRecordingStatus('paused');
      editorLog('gif recording pause: status=paused');
    } catch (recordingError) {
      editorLog(`gif recording pause failed: ${String(recordingError)}`);
      setError(String(recordingError));
    }
  }

  async function handleGifRecordingFinish() {
    editorLog(
      `gif recording finish click: session=${sessionId || '<missing>'} status=${gifRecordingStatus}`,
    );
    if (!sessionId || gifRecordingStatus === 'idle' || gifRecordingStatus === 'saving') {
      editorLog('gif recording finish ignored: missing session or idle/saving');
      return;
    }
    setError('');
    setGifRecordingStatus('saving');
    isFinishedRef.current = true;
    editorLog('gif recording finish: status=saving finishedRef=true');
    try {
      editorLog('gif recording finish: invoke finish_gif_recording start');
      await invoke('finish_gif_recording', { sessionId });
      editorLog('gif recording finish: invoke finish_gif_recording complete');
      editorLog('gif recording finish: invoke cancel_capture_edit start');
      await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
      editorLog('gif recording finish: invoke cancel_capture_edit complete');
      editorLog('gif recording finish: close window start');
      await getCurrentWindow().close();
    } catch (recordingError) {
      editorLog(`gif recording finish failed: ${String(recordingError)}`);
      isFinishedRef.current = false;
      setGifRecordingStatus('paused');
      setError(String(recordingError));
    }
  }

  async function handleGifRecordingCancel() {
    editorLog(
      `gif recording cancel click: session=${sessionId || '<missing>'} status=${gifRecordingStatus}`,
    );
    if (!sessionId) {
      editorLog('gif recording cancel ignored: missing session');
      return;
    }
    isClosingRef.current = true;
    isFinishedRef.current = true;
    editorLog('gif recording cancel: invoke cancel_gif_recording start');
    await invoke('cancel_gif_recording', { sessionId }).catch(() => undefined);
    editorLog('gif recording cancel: invoke cancel_gif_recording complete');
    await invoke('close_gif_recording_toolbar_window', { sessionId }).catch(() => undefined);
    editorLog('gif recording cancel: invoke cancel_capture_edit start');
    await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
    editorLog('gif recording cancel: invoke cancel_capture_edit complete');
    editorLog('gif recording cancel: close window start');
    await getCurrentWindow().close();
  }

  function handleLongEdit() {
    stopLongScroll();
    editorLog(
      `long edit click: active=${longScreenshot.active} preview_len=${longScreenshot.previewDataUrl.length} total=${longScreenshot.totalHeight}`,
    );
    void (async () => {
      try {
        if (!sessionId) {
          throw new Error('Long screenshot session is missing.');
        }
        editorLog(`long edit click: invoke open_long_capture_edit_window start label=${windowLabel}`);
        await invoke('open_long_capture_edit_window', { sessionId });
        editorLog(`long edit click: invoke open_long_capture_edit_window complete label=${windowLabel}`);
      } catch (editError) {
        editorLog(`long edit failed: ${String(editError)}`);
        setError(String(editError));
      }
    })();
  }

  function startLongScroll(direction: 'up' | 'down') {
    if (
      editorMode !== 'long-capture'
      || isSaving
      || !imageLoaded
      || !sessionId
      || longScrollActiveRef.current
      || longScrollInFlightRef.current
    ) {
      return;
    }
    longScrollActiveRef.current = true;
    longScrollInFlightRef.current = true;
    editorLog(`long scroll ${direction}: start`);
    void invoke('scroll_long_capture', { sessionId, direction })
      .catch((scrollError: unknown) => {
        editorLog(`long scroll ${direction} failed: ${String(scrollError)}`);
        setError(String(scrollError));
        longScrollActiveRef.current = false;
      })
      .finally(() => {
        longScrollInFlightRef.current = false;
      });
  }

  function stopLongScroll() {
    if (!longScrollActiveRef.current || !sessionId) {
      return;
    }
    longScrollActiveRef.current = false;
    void invoke('stop_long_capture_scroll', { sessionId }).catch(() => undefined);
    editorLog('long scroll: stop');
  }

  async function resizeWindowForLongEdit(
    imageWidth: number,
    imageHeight: number,
    showAfterResize = false,
  ) {
    try {
      const appWindow = getCurrentWindow();
      const monitor = await currentMonitor();
      const scaleFactor = monitor?.scaleFactor ?? 1;
      const monitorWidth = monitor ? monitor.size.width / scaleFactor : window.screen.availWidth;
      const monitorHeight = monitor ? monitor.size.height / scaleFactor : window.screen.availHeight;
      const monitorLeft = monitor ? monitor.position.x / scaleFactor : 0;
      const monitorTop = monitor ? monitor.position.y / scaleFactor : 0;
      const maxWindowWidth = Math.max(1, monitorWidth - 80);
      const maxWindowHeight = Math.max(longEditToolbarHeight + 1, Math.min(900, monitorHeight - 80));
      const minWindowHeight = Math.min(maxWindowHeight, longEditMinWindowHeight);
      const verticalChromeHeight = longEditToolbarHeight + longEditPanelClearance * 2;
      const imageLogicalWidth = Math.max(1, imageWidth / scaleFactor);
      const imageLogicalHeight = Math.max(1, imageHeight / scaleFactor);
      const fitScale = Math.min(
        1,
        maxWindowWidth / imageLogicalWidth,
        Math.max(1, maxWindowHeight - verticalChromeHeight) / imageLogicalHeight,
      );
      const imageDisplayWidth = Math.max(1, Math.round(imageLogicalWidth * fitScale));
      const imageDisplayHeight = Math.max(1, Math.round(imageLogicalHeight * fitScale));
      const width = Math.min(maxWindowWidth, Math.max(longEditToolbarMinWidth, imageDisplayWidth));
      const height = Math.min(
        maxWindowHeight,
        Math.max(minWindowHeight, verticalChromeHeight + imageDisplayHeight),
      );
      setLongEditDisplaySize({ width: imageDisplayWidth, height: imageDisplayHeight });
      await appWindow.setSize(new LogicalSize(width, height));
      await appWindow.setPosition(new LogicalPosition(
        monitorLeft + (monitorWidth - width) / 2,
        monitorTop + (monitorHeight - height) / 2,
      ));
      if (showAfterResize) {
        editorLog(`long edit resize: show window label=${windowLabel} size=${width}x${height}`);
        await appWindow.show();
      }
      await appWindow.setFocus();
    } catch (resizeError) {
      editorLog(`long edit resize failed: ${String(resizeError)}`);
    }
  }

  async function handleCancel() {
    stopLongScroll();
    if (!sessionId) {
      return;
    }
    if (isFinishedRef.current) {
      return;
    }
    editorLog(`cancel click: start label=${windowLabel} mode=${editorMode} long_edit=${isLongEditLaunch}`);
    isClosingRef.current = true;
    isFinishedRef.current = true;
    if (editorMode === 'long-capture') {
      editorLog('cancel click: invoke cancel_long_capture start');
      await invoke('cancel_long_capture', { sessionId }).catch(() => undefined);
      editorLog('cancel click: invoke cancel_long_capture complete');
    }
    if (editorMode === 'recording') {
      editorLog('gif recording generic cancel: invoke cancel_gif_recording start');
      await invoke('cancel_gif_recording', { sessionId }).catch(() => undefined);
      editorLog('gif recording generic cancel: invoke cancel_gif_recording complete');
      await invoke('close_gif_recording_toolbar_window', { sessionId }).catch(() => undefined);
    }
    editorLog(`cancel click: invoke cancel_capture_edit start label=${windowLabel}`);
    await invoke('cancel_capture_edit', { sessionId }).catch((cancelError: unknown) => {
      editorLog(`cancel click: failed ${String(cancelError)}`);
      setError(String(cancelError));
    });
    editorLog(`cancel click: invoke cancel_capture_edit complete label=${windowLabel}`);
    editorLog(`cancel click: close window label=${windowLabel}`);
    await getCurrentWindow().close();
  }

  async function handleConfirm() {
    stopLongScroll();
    if (!imageRef.current || !sessionId) {
      return;
    }
    if (isFinishedRef.current) {
      return;
    }

    editorLog(`confirm click: start label=${windowLabel} mode=${editorMode} long_edit=${isLongEditLaunch}`);
    setIsSaving(true);
    setError('');
    isFinishedRef.current = true;
    try {
      editorLog('confirm click: encode png data url start');
      if (editorMode === 'long-capture') {
        editorLog('confirm click: invoke confirm_long_capture start');
        await invoke('confirm_long_capture', { sessionId });
        editorLog('confirm click: invoke confirm_long_capture complete');
      } else {
        const pngBase64 = buildEditedPngBase64();
        editorLog('confirm click: encode png data url complete');
        editorLog(`confirm click: invoke confirm_regular_capture_edit start label=${windowLabel}`);
        await invoke('confirm_regular_capture_edit', { sessionId, pngBase64 });
        editorLog(`confirm click: invoke confirm_regular_capture_edit complete label=${windowLabel}`);
      }
      editorLog(`confirm click: close window label=${windowLabel}`);
      await getCurrentWindow().close();
    } catch (saveError) {
      isFinishedRef.current = false;
      editorLog(`confirm click: failed ${String(saveError)}`);
      setError(String(saveError));
      setIsSaving(false);
    }
  }

  const thumbnailMaxHeight = longScreenshotThumbnailHeightFromQuery
    || Math.max(96, window.innerHeight - 16);
  const thumbnailHeight = Math.min(
    thumbnailMaxHeight,
    Math.max(
      96,
      (longScreenshotThumbnailWidth / Math.max(imageSize.width, 1))
        * Math.max(imageSize.height, longScreenshot.totalHeight || imageSize.height),
    ),
  );
  const thumbnailLeft = Number.isFinite(longScreenshotThumbnailLeftFromQuery)
    ? longScreenshotThumbnailLeftFromQuery
    : 8;
  const thumbnailAnchorTop = Number.isFinite(longScreenshotThumbnailTopFromQuery)
    ? longScreenshotThumbnailTopFromQuery
    : 8;
  const thumbnailRegionTop = Number.isFinite(longScreenshotThumbnailRegionTopFromQuery)
    ? longScreenshotThumbnailRegionTopFromQuery
    : 8;
  const thumbnailTop = Math.max(
    thumbnailRegionTop,
    Math.min(thumbnailAnchorTop, window.innerHeight - thumbnailHeight - 8),
  );
  const thumbnailTotalHeight = Math.max(longScreenshot.totalHeight || imageSize.height, 1);
  const thumbnailViewportHeight = Math.min(
    longScreenshot.frameHeight || imageSize.height,
    thumbnailTotalHeight,
  );
  const thumbnailViewportTop = Math.max(
    0,
    Math.min(longScreenshot.scrollOffset, thumbnailTotalHeight - thumbnailViewportHeight),
  );
  const thumbnailViewportHeightPercent =
    (thumbnailViewportHeight / thumbnailTotalHeight) * 100;
  const thumbnailViewportTopPercent = (thumbnailViewportTop / thumbnailTotalHeight) * 100;

  return (
    <div
      className="relative h-screen overflow-hidden text-on-surface"
      style={{ backgroundColor: 'transparent' }}
    >
      {!isPreload && (
        <>
          {isLongEditWindow ? (
            <div className="absolute inset-0 z-50 flex flex-col overflow-hidden bg-surface-container-lowest text-on-surface">
              <div className="flex h-12 shrink-0 items-center justify-center border-b border-outline-variant/40 bg-surface-container-lowest px-3 shadow-sm">
                <ScreenshotEditorToolbar
                  toolbarRef={toolbarRef}
                  toolbarPosition={toolbarPosition}
                  editorVisible={editorVisible}
                  tool={tool}
                  onToolClick={handleToolClick}
                  popupPositionClass="top-[calc(100%+8px)]"
                  lineWidth={lineWidth}
                  setLineWidth={setLineWidth}
                  mosaicWidth={mosaicWidth}
                  setMosaicWidth={setMosaicWidth}
                  mosaicSize={mosaicSize}
                  setMosaicSize={setMosaicSize}
                  fontSize={fontSize}
                  setFontSize={setFontSize}
                  emojiPickerOpen={emojiPickerOpen}
                  visibleEmojiChoices={visibleEmojiChoices}
                  onEmojiChoice={handleEmojiChoice}
                  emojiPage={emojiPage}
                  emojiPageCount={emojiPageCount}
                  emojiCount={emojiChoices.length}
                  setEmojiPage={setEmojiPage}
                  color={color}
                  colorPickerOpen={colorPickerOpen}
                  setColorPickerOpen={setColorPickerOpen}
                  setEmojiPickerOpen={setEmojiPickerOpen}
                  handleColorSquarePointer={handleColorSquarePointer}
                  handleColorChange={handleColorChange}
                  hexInput={hexInput}
                  setHexInput={setHexInput}
                  handleHexInputCommit={handleHexInputCommit}
                  hexToHsv={hexToHsv}
                  hsvToHex={hsvToHex}
                  undo={undo}
                  redo={redo}
                  canUndo={undoStack.length > 0}
                  canRedo={redoStack.length > 0}
                  clearAnnotations={() => {
                    setSelectedAnnotationIndex(null);
                    commitAnnotations([]);
                  }}
                  onLongCapture={handleLongCaptureStart}
                  onGifRecording={handleGifRecordingModeStart}
                  onCancel={() => void handleCancel()}
                  onConfirm={() => void handleConfirm()}
                  isSaving={isSaving}
                  imageLoaded={imageLoaded}
                  embedded
                  showLongCapture={false}
                />
              </div>
              <div
                className="flex-1 overflow-hidden bg-surface-container-lowest"
                style={{
                  paddingTop: longEditPanelClearance,
                  paddingBottom: longEditPanelClearance,
                }}
              >
                <div
                  className="relative mx-auto bg-surface-container-lowest"
                  style={{
                    width: longEditDisplaySize.width,
                    height: longEditDisplaySize.height,
                  }}
                >
                  <ScreenshotEditorCanvas
                    selectionOffset={{ left: 0, top: 0 }}
                    displaySize={longEditDisplaySize}
                    editorVisible={editorVisible}
                    screenshotDataUrl={screenshotDataUrl}
                    baseCanvasRef={baseCanvasRef}
                    draftCanvasRef={draftCanvasRef}
                    imageSize={imageSize}
                    onPointerDown={handlePointerDown}
                    onPointerMove={handlePointerMove}
                    onPointerUp={handlePointerUp}
                    onPointerCancel={() => {
                      annotationDragRef.current = null;
                      draftRef.current = null;
                      dragStartRef.current = null;
                      setIsDragging(false);
                      setCanvasCursor(tool ? 'crosshair' : 'default');
                      renderDraft(null);
                    }}
                    onPointerLeave={() => {
                      if (!isDragging) {
                        setCanvasCursor(tool ? 'crosshair' : 'default');
                      }
                    }}
                    canvasCursor={canvasCursor}
                    selectedControlRect={selectedControlRect}
                    canvasRectToCss={canvasRectToCss}
                    resizeHandles={resizeHandles}
                    textDraft={textDraft}
                    textAreaRef={textAreaRef}
                    setTextDraft={setTextDraft}
                    commitTextDraft={commitTextDraft}
                    color={color}
                    fontSize={fontSize}
                    viewportImageOffsetY={0}
                    showAnnotationLayer
                    framed={false}
                  />
                </div>
              </div>
            </div>
          ) : (
            <>
              <ScreenshotEditorCanvas
                selectionOffset={selectionOffset}
                displaySize={displaySize}
                editorVisible={editorVisible && (editorMode === 'edit' || editorMode === 'recording')}
                screenshotDataUrl={editorMode === 'edit' ? screenshotDataUrl : ''}
                baseCanvasRef={baseCanvasRef}
                draftCanvasRef={draftCanvasRef}
                imageSize={imageSize}
                onPointerDown={handlePointerDown}
                onPointerMove={handlePointerMove}
                onPointerUp={handlePointerUp}
                onPointerCancel={() => {
                  annotationDragRef.current = null;
                  draftRef.current = null;
                  dragStartRef.current = null;
                  setIsDragging(false);
                  setCanvasCursor(tool ? 'crosshair' : 'default');
                  renderDraft(null);
                }}
                onPointerLeave={() => {
                  if (!isDragging) {
                    setCanvasCursor(tool ? 'crosshair' : 'default');
                  }
                }}
                canvasCursor={canvasCursor}
                selectedControlRect={selectedControlRect}
                canvasRectToCss={canvasRectToCss}
                resizeHandles={resizeHandles}
                textDraft={textDraft}
                textAreaRef={textAreaRef}
                setTextDraft={setTextDraft}
                commitTextDraft={commitTextDraft}
                color={color}
                fontSize={fontSize}
                viewportImageOffsetY={0}
                showAnnotationLayer={editorMode === 'edit'}
                insetFrame={editorMode === 'recording'}
              />

              {editorMode === 'edit' ? (
                <ScreenshotEditorToolbar
                  toolbarRef={toolbarRef}
                  toolbarPosition={toolbarPosition}
                  editorVisible={editorVisible}
                  tool={tool}
                  onToolClick={handleToolClick}
                  popupPositionClass={popupPositionClass}
                  lineWidth={lineWidth}
                  setLineWidth={setLineWidth}
                  mosaicWidth={mosaicWidth}
                  setMosaicWidth={setMosaicWidth}
                  mosaicSize={mosaicSize}
                  setMosaicSize={setMosaicSize}
                  fontSize={fontSize}
                  setFontSize={setFontSize}
                  emojiPickerOpen={emojiPickerOpen}
                  visibleEmojiChoices={visibleEmojiChoices}
                  onEmojiChoice={handleEmojiChoice}
                  emojiPage={emojiPage}
                  emojiPageCount={emojiPageCount}
                  emojiCount={emojiChoices.length}
                  setEmojiPage={setEmojiPage}
                  color={color}
                  colorPickerOpen={colorPickerOpen}
                  setColorPickerOpen={setColorPickerOpen}
                  setEmojiPickerOpen={setEmojiPickerOpen}
                  handleColorSquarePointer={handleColorSquarePointer}
                  handleColorChange={handleColorChange}
                  hexInput={hexInput}
                  setHexInput={setHexInput}
                  handleHexInputCommit={handleHexInputCommit}
                  hexToHsv={hexToHsv}
                  hsvToHex={hsvToHex}
                  undo={undo}
                  redo={redo}
                  canUndo={undoStack.length > 0}
                  canRedo={redoStack.length > 0}
                  clearAnnotations={() => {
                    setSelectedAnnotationIndex(null);
                    commitAnnotations([]);
                  }}
                  onLongCapture={handleLongCaptureStart}
                  onGifRecording={handleGifRecordingModeStart}
                  onCancel={() => void handleCancel()}
                  onConfirm={() => void handleConfirm()}
                  isSaving={isSaving}
                  imageLoaded={imageLoaded}
                  showLongCapture={!isLinux}
                  useNativeTooltip={isWindows}
                />
              ) : editorMode === 'long-capture' ? (
                <LongScreenshotToolbar
                  toolbarRef={toolbarRef}
                  toolbarPosition={toolbarPosition}
                  editorVisible={editorVisible}
                  onEdit={handleLongEdit}
                  onScrollStart={startLongScroll}
                  onScrollStop={stopLongScroll}
                  onCancel={() => void handleCancel()}
                  onConfirm={() => void handleConfirm()}
                  isSaving={isSaving}
                  imageLoaded={imageLoaded}
                  useNativeTooltip={isWindows}
                />
              ) : editorMode === 'recording' ? (
                <GifRecordingToolbar
                  toolbarRef={toolbarRef}
                  toolbarPosition={toolbarPosition}
                  editorVisible={editorVisible}
                  status={gifRecordingStatus}
                  onStart={() => void handleGifRecordingStart()}
                  onPause={() => void handleGifRecordingPause()}
                  onFinish={() => void handleGifRecordingFinish()}
                  onCancel={() => void handleGifRecordingCancel()}
                  useNativeTooltip={isWindows}
                />
              ) : null}

              {editorMode === 'long-capture' && longScreenshot.previewSegments.length > 0 && (
                <div
                  className="absolute z-30 overflow-hidden rounded-md border border-outline-variant/50 bg-surface-container-lowest shadow-xl"
                  style={{
                    left: thumbnailLeft,
                    top: thumbnailTop,
                    width: longScreenshotThumbnailWidth,
                    height: thumbnailHeight,
                  }}
                >
                  <div className="h-full w-full">
                    {longScreenshot.previewSegments.map((segment) => (
                      <img
                        key={segment.id}
                        src={segment.dataUrl}
                        alt=""
                        draggable={false}
                        className="block w-full select-none object-fill"
                        style={{
                          height: `${segment.rows / Math.max(longScreenshot.totalHeight || segment.rows, 1) * 100}%`,
                        }}
                      />
                    ))}
                  </div>
                  <div
                    className="absolute left-0 right-0 border-2 border-primary/90 bg-primary/10"
                    style={{
                      top: `${thumbnailViewportTopPercent}%`,
                      height: `${thumbnailViewportHeightPercent}%`,
                    }}
                  />
                </div>
              )}
            </>
          )}

      {error && <div className="fixed bottom-16 left-2 right-2 rounded bg-error/90 px-3 py-2 text-sm text-white">{error}</div>}
        </>
      )}
    </div>
  );
}

function drawAnnotation(context: CanvasRenderingContext2D, annotation: Annotation) {
  context.save();
  context.lineCap = 'round';
  context.lineJoin = 'round';

  if (annotation.kind === 'pen') {
    drawPath(context, annotation.points, annotation.color, annotation.width);
  } else if (annotation.kind === 'line') {
    context.strokeStyle = annotation.color;
    context.lineWidth = annotation.width;
    context.beginPath();
    context.moveTo(annotation.from.x, annotation.from.y);
    context.lineTo(annotation.to.x, annotation.to.y);
    context.stroke();
  } else if (annotation.kind === 'arrow') {
    drawArrow(context, annotation);
  } else if (annotation.kind === 'ellipse') {
    context.strokeStyle = annotation.color;
    context.lineWidth = annotation.width;
    context.beginPath();
    context.ellipse(
      annotation.rect.x + annotation.rect.width / 2,
      annotation.rect.y + annotation.rect.height / 2,
      Math.abs(annotation.rect.width / 2),
      Math.abs(annotation.rect.height / 2),
      0,
      0,
      Math.PI * 2,
    );
    context.stroke();
  } else if (annotation.kind === 'rect') {
    context.strokeStyle = annotation.color;
    context.lineWidth = annotation.width;
    context.strokeRect(annotation.rect.x, annotation.rect.y, annotation.rect.width, annotation.rect.height);
  } else if (annotation.kind === 'mosaic') {
    drawMosaicPath(context, annotation.points, annotation.width, annotation.blockSize);
  } else if (annotation.kind === 'text') {
    context.fillStyle = annotation.color;
    context.font = `700 ${annotation.fontSize}px Inter, sans-serif`;
    context.textBaseline = 'top';
    for (const [index, line] of annotation.text.split('\n').entries()) {
      context.fillText(line, annotation.position.x, annotation.position.y + index * annotation.fontSize * 1.25);
    }
  } else if (annotation.kind === 'emoji') {
    drawEmojiAnnotation(context, annotation);
  }

  context.restore();
}

function drawEmojiAnnotation(context: CanvasRenderingContext2D, annotation: Extract<Annotation, { kind: 'emoji' }>) {
  context.font = `${annotation.rect.height}px "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif`;
  context.textAlign = 'left';
  context.textBaseline = 'alphabetic';

  const metrics = context.measureText(annotation.emoji);
  const left = metrics.actualBoundingBoxLeft || 0;
  const right = metrics.actualBoundingBoxRight || metrics.width;
  const ascent = metrics.actualBoundingBoxAscent || annotation.rect.height * 0.8;
  const descent = metrics.actualBoundingBoxDescent || annotation.rect.height * 0.2;
  const centerX = annotation.rect.x + annotation.rect.width / 2;
  const centerY = annotation.rect.y + annotation.rect.height / 2;

  context.fillText(
    annotation.emoji,
    centerX - (right - left) / 2,
    centerY + (ascent - descent) / 2,
  );
}

function isSelectableAnnotation(annotation: Annotation | undefined): annotation is Exclude<Annotation, { kind: 'mosaic' }> {
  return Boolean(annotation) && annotation?.kind !== 'mosaic';
}

function hitSelectableAnnotation(point: Point, annotations: Annotation[]) {
  for (let index = annotations.length - 1; index >= 0; index -= 1) {
    const annotation = annotations[index];
    if (isSelectableAnnotation(annotation) && pointInRect(point, expandRectToMinSize(getAnnotationBounds(annotation), selectionMinSize))) {
      return index;
    }
  }
  return null;
}

function hitResizeHandle(point: Point, rect: Rect, scale: { x: number; y: number }): ResizeHandle | null {
  const handles = handlePoints(rect);
  const radius = Math.max(10, emojiHandleSize / Math.max(Math.min(scale.x, scale.y), 0.001));
  for (const handle of handles) {
    if (Math.abs(point.x - handle.point.x) <= radius && Math.abs(point.y - handle.point.y) <= radius) {
      return handle.id;
    }
  }
  return null;
}

function getResizeCursor(handle: ResizeHandle) {
  if (handle === 'nw' || handle === 'se') {
    return 'nwse-resize';
  }
  if (handle === 'ne' || handle === 'sw') {
    return 'nesw-resize';
  }
  if (handle === 'n' || handle === 's') {
    return 'ns-resize';
  }
  return 'ew-resize';
}

function handlePoints(rect: Rect): Array<{ id: ResizeHandle; point: Point }> {
  const centerX = rect.x + rect.width / 2;
  const centerY = rect.y + rect.height / 2;
  const right = rect.x + rect.width;
  const bottom = rect.y + rect.height;
  return [
    { id: 'nw', point: { x: rect.x, y: rect.y } },
    { id: 'n', point: { x: centerX, y: rect.y } },
    { id: 'ne', point: { x: right, y: rect.y } },
    { id: 'e', point: { x: right, y: centerY } },
    { id: 'se', point: { x: right, y: bottom } },
    { id: 's', point: { x: centerX, y: bottom } },
    { id: 'sw', point: { x: rect.x, y: bottom } },
    { id: 'w', point: { x: rect.x, y: centerY } },
  ];
}

function resizeHandles(rect: Rect): Array<{ id: ResizeHandle; x: number; y: number }> {
  return [
    { id: 'nw', x: 0, y: 0 },
    { id: 'n', x: rect.width / 2, y: 0 },
    { id: 'ne', x: rect.width, y: 0 },
    { id: 'e', x: rect.width, y: rect.height / 2 },
    { id: 'se', x: rect.width, y: rect.height },
    { id: 's', x: rect.width / 2, y: rect.height },
    { id: 'sw', x: 0, y: rect.height },
    { id: 'w', x: 0, y: rect.height / 2 },
  ];
}

function getAnnotationBounds(annotation: Exclude<Annotation, { kind: 'mosaic' }>): Rect {
  if (annotation.kind === 'pen') {
    return expandRect(pointsBounds(annotation.points), annotation.width / 2);
  }
  if (annotation.kind === 'line' || annotation.kind === 'arrow') {
    return expandRect(pointsBounds([annotation.from, annotation.to]), annotation.width / 2);
  }
  if (annotation.kind === 'ellipse' || annotation.kind === 'rect') {
    return expandRect(normalizeRect(annotation.rect), annotation.width / 2);
  }
  if (annotation.kind === 'text') {
    return getTextBounds(annotation);
  }
  return normalizeRect(annotation.rect);
}

function pointsBounds(points: Point[]): Rect {
  if (points.length === 0) {
    return { x: 0, y: 0, width: 1, height: 1 };
  }
  const xs = points.map((point) => point.x);
  const ys = points.map((point) => point.y);
  const left = Math.min(...xs);
  const top = Math.min(...ys);
  const right = Math.max(...xs);
  const bottom = Math.max(...ys);
  return {
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  };
}

function getTextBounds(annotation: Extract<Annotation, { kind: 'text' }>): Rect {
  const lines = annotation.text.split('\n');
  const longestLine = lines.reduce((longest, line) => Math.max(longest, line.length), 1);
  return {
    x: annotation.position.x,
    y: annotation.position.y,
    width: Math.max(annotation.fontSize, longestLine * annotation.fontSize * 0.62),
    height: Math.max(annotation.fontSize, lines.length * annotation.fontSize * 1.25),
  };
}

function moveAnnotation(annotation: Exclude<Annotation, { kind: 'mosaic' }>, delta: Point, bounds: { width: number; height: number }): Annotation {
  const clampedDelta = clampDeltaForRect(getAnnotationBounds(annotation), delta, bounds);
  if (annotation.kind === 'pen') {
    return { ...annotation, points: annotation.points.map((point) => addPoints(point, clampedDelta)) };
  }
  if (annotation.kind === 'line' || annotation.kind === 'arrow') {
    return {
      ...annotation,
      from: addPoints(annotation.from, clampedDelta),
      to: addPoints(annotation.to, clampedDelta),
    };
  }
  if (annotation.kind === 'ellipse' || annotation.kind === 'rect') {
    return {
      ...annotation,
      rect: {
        ...annotation.rect,
        x: annotation.rect.x + clampedDelta.x,
        y: annotation.rect.y + clampedDelta.y,
      },
    };
  }
  if (annotation.kind === 'text') {
    return { ...annotation, position: addPoints(annotation.position, clampedDelta) };
  }
  return { ...annotation, rect: moveRect(annotation.rect, clampedDelta, bounds) };
}

function resizeAnnotation(
  annotation: Exclude<Annotation, { kind: 'mosaic' }>,
  initialRect: Rect,
  delta: Point,
  handle: ResizeHandle,
  bounds: { width: number; height: number },
): Annotation {
  if (annotation.kind === 'emoji') {
    return { ...annotation, rect: resizeRect(initialRect, delta, handle, bounds) };
  }

  const targetRect = resizeSelectionRect(initialRect, delta, handle, bounds);
  return transformAnnotation(annotation, initialRect, targetRect);
}

function transformAnnotation(annotation: Exclude<Annotation, { kind: 'mosaic' | 'emoji' }>, fromRect: Rect, toRect: Rect): Annotation {
  const transformPoint = (point: Point) => transformPointBetweenRects(point, fromRect, toRect);

  if (annotation.kind === 'pen') {
    const scale = averageRectScale(fromRect, toRect);
    return {
      ...annotation,
      points: annotation.points.map(transformPoint),
      width: Math.max(1, annotation.width * scale),
    };
  }
  if (annotation.kind === 'line' || annotation.kind === 'arrow') {
    return {
      ...annotation,
      from: transformPoint(annotation.from),
      to: transformPoint(annotation.to),
      width: Math.max(1, annotation.width * averageRectScale(fromRect, toRect)),
    };
  }
  if (annotation.kind === 'ellipse' || annotation.kind === 'rect') {
    const topLeft = transformPoint({ x: annotation.rect.x, y: annotation.rect.y });
    const bottomRight = transformPoint({
      x: annotation.rect.x + annotation.rect.width,
      y: annotation.rect.y + annotation.rect.height,
    });
    return {
      ...annotation,
      rect: {
        x: topLeft.x,
        y: topLeft.y,
        width: bottomRight.x - topLeft.x,
        height: bottomRight.y - topLeft.y,
      },
      width: Math.max(1, annotation.width * averageRectScale(fromRect, toRect)),
    };
  }

  return {
    ...annotation,
    position: { x: toRect.x, y: toRect.y },
    fontSize: Math.max(8, annotation.fontSize * averageRectScale(fromRect, toRect)),
  };
}

function transformPointBetweenRects(point: Point, fromRect: Rect, toRect: Rect): Point {
  const normalizedFrom = expandRectToMinSize(normalizeRect(fromRect), 1);
  const normalizedTo = normalizeRect(toRect);
  const xRatio = Math.abs(normalizedFrom.width) < 1 ? 0.5 : (point.x - normalizedFrom.x) / normalizedFrom.width;
  const yRatio = Math.abs(normalizedFrom.height) < 1 ? 0.5 : (point.y - normalizedFrom.y) / normalizedFrom.height;
  return {
    x: normalizedTo.x + xRatio * normalizedTo.width,
    y: normalizedTo.y + yRatio * normalizedTo.height,
  };
}

function resizeSelectionRect(rect: Rect, delta: Point, handle: ResizeHandle, bounds: { width: number; height: number }): Rect {
  let left = rect.x;
  let top = rect.y;
  let right = rect.x + rect.width;
  let bottom = rect.y + rect.height;

  if (handle.includes('w')) {
    left += delta.x;
  }
  if (handle.includes('e')) {
    right += delta.x;
  }
  if (handle.includes('n')) {
    top += delta.y;
  }
  if (handle.includes('s')) {
    bottom += delta.y;
  }

  if (right - left < selectionMinSize) {
    if (handle.includes('w')) {
      left = right - selectionMinSize;
    } else {
      right = left + selectionMinSize;
    }
  }
  if (bottom - top < selectionMinSize) {
    if (handle.includes('n')) {
      top = bottom - selectionMinSize;
    } else {
      bottom = top + selectionMinSize;
    }
  }

  const width = right - left;
  const height = bottom - top;
  left = Math.min(Math.max(left, 0), Math.max(bounds.width - width, 0));
  top = Math.min(Math.max(top, 0), Math.max(bounds.height - height, 0));

  return { x: left, y: top, width, height };
}

function normalizeRect(rect: Rect): Rect {
  const left = Math.min(rect.x, rect.x + rect.width);
  const top = Math.min(rect.y, rect.y + rect.height);
  return {
    x: left,
    y: top,
    width: Math.abs(rect.width),
    height: Math.abs(rect.height),
  };
}

function expandRect(rect: Rect, amount: number): Rect {
  return {
    x: rect.x - amount,
    y: rect.y - amount,
    width: rect.width + amount * 2,
    height: rect.height + amount * 2,
  };
}

function expandRectToMinSize(rect: Rect, minSize: number): Rect {
  const normalized = normalizeRect(rect);
  const width = Math.max(normalized.width, minSize);
  const height = Math.max(normalized.height, minSize);
  return {
    x: normalized.x + normalized.width / 2 - width / 2,
    y: normalized.y + normalized.height / 2 - height / 2,
    width,
    height,
  };
}

function clampDeltaForRect(rect: Rect, delta: Point, bounds: { width: number; height: number }): Point {
  const normalized = normalizeRect(rect);
  return {
    x: Math.min(Math.max(delta.x, -normalized.x), bounds.width - normalized.x - normalized.width),
    y: Math.min(Math.max(delta.y, -normalized.y), bounds.height - normalized.y - normalized.height),
  };
}

function addPoints(point: Point, delta: Point): Point {
  return { x: point.x + delta.x, y: point.y + delta.y };
}

function averageRectScale(fromRect: Rect, toRect: Rect) {
  const xScale = toRect.width / Math.max(fromRect.width, 1);
  const yScale = toRect.height / Math.max(fromRect.height, 1);
  return Math.max(0.1, (Math.abs(xScale) + Math.abs(yScale)) / 2);
}

function pointInRect(point: Point, rect: Rect) {
  const left = Math.min(rect.x, rect.x + rect.width);
  const right = Math.max(rect.x, rect.x + rect.width);
  const top = Math.min(rect.y, rect.y + rect.height);
  const bottom = Math.max(rect.y, rect.y + rect.height);
  return point.x >= left && point.x <= right && point.y >= top && point.y <= bottom;
}

function moveRect(rect: Rect, delta: Point, bounds: { width: number; height: number }): Rect {
  return {
    ...rect,
    x: Math.min(Math.max(rect.x + delta.x, 0), Math.max(bounds.width - rect.width, 0)),
    y: Math.min(Math.max(rect.y + delta.y, 0), Math.max(bounds.height - rect.height, 0)),
  };
}

function resizeRect(rect: Rect, delta: Point, handle: ResizeHandle, bounds: { width: number; height: number }): Rect {
  const size = Math.max(rect.width, rect.height);
  const center = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  let nextSize = size;
  let nextX = rect.x;
  let nextY = rect.y;

  if (handle === 'e' || handle === 'w') {
    nextSize = Math.max(emojiMinSize, size + (handle === 'e' ? delta.x : -delta.x));
    nextX = handle === 'e' ? rect.x : rect.x + rect.width - nextSize;
    nextY = center.y - nextSize / 2;
  } else if (handle === 's' || handle === 'n') {
    nextSize = Math.max(emojiMinSize, size + (handle === 's' ? delta.y : -delta.y));
    nextX = center.x - nextSize / 2;
    nextY = handle === 's' ? rect.y : rect.y + rect.height - nextSize;
  } else {
    const horizontalDelta = handle.includes('e') ? delta.x : -delta.x;
    const verticalDelta = handle.includes('s') ? delta.y : -delta.y;
    nextSize = Math.max(emojiMinSize, size + Math.max(horizontalDelta, verticalDelta));
    nextX = handle.includes('e') ? rect.x : rect.x + rect.width - nextSize;
    nextY = handle.includes('s') ? rect.y : rect.y + rect.height - nextSize;
  }

  nextSize = Math.min(nextSize, bounds.width, bounds.height);
  nextX = Math.min(Math.max(nextX, 0), Math.max(bounds.width - nextSize, 0));
  nextY = Math.min(Math.max(nextY, 0), Math.max(bounds.height - nextSize, 0));

  return {
    x: nextX,
    y: nextY,
    width: nextSize,
    height: nextSize,
  };
}

function sameRect(left: Rect, right: Rect) {
  return (
    Math.abs(left.x - right.x) < 0.5
    && Math.abs(left.y - right.y) < 0.5
    && Math.abs(left.width - right.width) < 0.5
    && Math.abs(left.height - right.height) < 0.5
  );
}

function drawPath(context: CanvasRenderingContext2D, points: Point[], color: string, width: number) {
  if (points.length < 2) {
    return;
  }
  context.strokeStyle = color;
  context.lineWidth = width;
  context.beginPath();
  context.moveTo(points[0].x, points[0].y);
  for (const point of points.slice(1)) {
    context.lineTo(point.x, point.y);
  }
  context.stroke();
}

function drawArrow(context: CanvasRenderingContext2D, annotation: Extract<Annotation, { kind: 'arrow' }>) {
  const angle = Math.atan2(annotation.to.y - annotation.from.y, annotation.to.x - annotation.from.x);
  const headLength = Math.max(12, annotation.width * 4);

  context.strokeStyle = annotation.color;
  context.fillStyle = annotation.color;
  context.lineWidth = annotation.width;
  context.beginPath();
  context.moveTo(annotation.from.x, annotation.from.y);
  context.lineTo(annotation.to.x, annotation.to.y);
  context.stroke();

  context.beginPath();
  context.moveTo(annotation.to.x, annotation.to.y);
  context.lineTo(
    annotation.to.x - headLength * Math.cos(angle - Math.PI / 6),
    annotation.to.y - headLength * Math.sin(angle - Math.PI / 6),
  );
  context.lineTo(
    annotation.to.x - headLength * Math.cos(angle + Math.PI / 6),
    annotation.to.y - headLength * Math.sin(angle + Math.PI / 6),
  );
  context.closePath();
  context.fill();
}

function drawMosaicPath(context: CanvasRenderingContext2D, points: Point[], width: number, blockSize: number) {
  for (const point of points) {
    drawMosaicRect(
      context,
      {
        x: point.x - width / 2,
        y: point.y - width / 2,
        width,
        height: width,
      },
      blockSize,
    );
  }
}

function drawMosaicRect(context: CanvasRenderingContext2D, rect: Rect, blockSize: number) {
  const x = Math.max(0, Math.floor(rect.x));
  const y = Math.max(0, Math.floor(rect.y));
  const width = Math.min(context.canvas.width - x, Math.floor(rect.width));
  const height = Math.min(context.canvas.height - y, Math.floor(rect.height));
  if (width <= 0 || height <= 0) {
    return;
  }

  const data = context.getImageData(x, y, width, height);
  for (let blockY = 0; blockY < height; blockY += blockSize) {
    for (let blockX = 0; blockX < width; blockX += blockSize) {
      const endX = Math.min(blockX + blockSize, width);
      const endY = Math.min(blockY + blockSize, height);
      let redSum = 0;
      let greenSum = 0;
      let blueSum = 0;
      let alphaSum = 0;
      let count = 0;

      for (let py = blockY; py < endY; py += 1) {
        for (let px = blockX; px < endX; px += 1) {
          const index = (py * width + px) * 4;
          redSum += data.data[index];
          greenSum += data.data[index + 1];
          blueSum += data.data[index + 2];
          alphaSum += data.data[index + 3];
          count += 1;
        }
      }

      const red = Math.round(redSum / count);
      const green = Math.round(greenSum / count);
      const blue = Math.round(blueSum / count);
      const alpha = Math.round(alphaSum / count);

      for (let py = blockY; py < endY; py += 1) {
        for (let px = blockX; px < endX; px += 1) {
          const index = (py * width + px) * 4;
          data.data[index] = red;
          data.data[index + 1] = green;
          data.data[index + 2] = blue;
          data.data[index + 3] = alpha;
        }
      }
    }
  }
  context.putImageData(data, x, y);
}

function rectFromPoints(from: Point, to: Point, square: boolean): Rect {
  let nextTo = to;
  if (square) {
    const size = Math.max(Math.abs(to.x - from.x), Math.abs(to.y - from.y));
    nextTo = {
      x: from.x + Math.sign(to.x - from.x || 1) * size,
      y: from.y + Math.sign(to.y - from.y || 1) * size,
    };
  }

  return {
    x: from.x,
    y: from.y,
    width: nextTo.x - from.x,
    height: nextTo.y - from.y,
  };
}

function constrainPoint(from: Point, to: Point): Point {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const angle = Math.atan2(dy, dx);
  const distance = Math.hypot(dx, dy);
  const snapped = Math.round(angle / (Math.PI / 4)) * (Math.PI / 4);
  return {
    x: from.x + Math.cos(snapped) * distance,
    y: from.y + Math.sin(snapped) * distance,
  };
}

function canvasSampleSummary(context: CanvasRenderingContext2D, canvas: HTMLCanvasElement) {
  const sampleColumns = Math.min(canvas.width, 16);
  const sampleRows = Math.min(canvas.height, 16);
  let totalSamples = 0;
  let nonWhiteSamples = 0;
  let brightSamples = 0;
  let transparentSamples = 0;
  let redTotal = 0;
  let greenTotal = 0;
  let blueTotal = 0;

  for (let row = 0; row < sampleRows; row += 1) {
    for (let column = 0; column < sampleColumns; column += 1) {
      const x = sampleColumns <= 1 ? 0 : Math.round((column * (canvas.width - 1)) / (sampleColumns - 1));
      const y = sampleRows <= 1 ? 0 : Math.round((row * (canvas.height - 1)) / (sampleRows - 1));
      const [red, green, blue, alpha] = context.getImageData(x, y, 1, 1).data;
      totalSamples += 1;
      redTotal += red;
      greenTotal += green;
      blueTotal += blue;
      if (alpha === 0) {
        transparentSamples += 1;
      }
      if (alpha !== 0 && (red < 245 || green < 245 || blue < 245)) {
        nonWhiteSamples += 1;
      }
      if (red >= 245 && green >= 245 && blue >= 245) {
        brightSamples += 1;
      }
    }
  }

  const averageRed = Math.round(redTotal / Math.max(totalSamples, 1));
  const averageGreen = Math.round(greenTotal / Math.max(totalSamples, 1));
  const averageBlue = Math.round(blueTotal / Math.max(totalSamples, 1));
  return `nonWhiteSamples=${nonWhiteSamples}/${totalSamples} brightSamples=${brightSamples}/${totalSamples} transparentSamples=${transparentSamples}/${totalSamples} avgRgb=${averageRed},${averageGreen},${averageBlue}`;
}

function isHexColor(value: string | undefined): value is string {
  return /^#[0-9a-fA-F]{6}$/.test(value ?? '');
}

function hexToHsv(hex: string): { h: number; s: number; v: number } {
  const normalized = isHexColor(hex) ? hex : defaultEditorColor;
  const red = Number.parseInt(normalized.slice(1, 3), 16) / 255;
  const green = Number.parseInt(normalized.slice(3, 5), 16) / 255;
  const blue = Number.parseInt(normalized.slice(5, 7), 16) / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const delta = max - min;
  let hue = 0;

  if (delta !== 0) {
    if (max === red) {
      hue = 60 * (((green - blue) / delta) % 6);
    } else if (max === green) {
      hue = 60 * ((blue - red) / delta + 2);
    } else {
      hue = 60 * ((red - green) / delta + 4);
    }
  }

  return {
    h: (hue + 360) % 360,
    s: max === 0 ? 0 : delta / max,
    v: max,
  };
}

function hsvToHex({ h, s, v }: { h: number; s: number; v: number }) {
  const chroma = v * s;
  const hue = ((h % 360) + 360) % 360;
  const x = chroma * (1 - Math.abs((hue / 60) % 2 - 1));
  const match = v - chroma;
  let red = 0;
  let green = 0;
  let blue = 0;

  if (hue < 60) {
    red = chroma;
    green = x;
  } else if (hue < 120) {
    red = x;
    green = chroma;
  } else if (hue < 180) {
    green = chroma;
    blue = x;
  } else if (hue < 240) {
    green = x;
    blue = chroma;
  } else if (hue < 300) {
    red = x;
    blue = chroma;
  } else {
    red = chroma;
    blue = x;
  }

  return rgbToHex(
    Math.round((red + match) * 255),
    Math.round((green + match) * 255),
    Math.round((blue + match) * 255),
  );
}

function rgbToHex(red: number, green: number, blue: number) {
  return `#${[red, green, blue]
    .map((value) => value.toString(16).padStart(2, '0'))
    .join('')}`;
}

async function bootstrapScreenshotEditor() {
  let initialLanguage = normalizeLanguage(navigator.language);

  try {
    const settings = await invoke<AppSettings>('get_app_settings');
    initialLanguage = normalizeLanguage(settings.interface_language);
  } catch {
    initialLanguage = normalizeLanguage(navigator.language);
  }

  await setupI18n(initialLanguage);
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<ScreenshotEditor />);
}

void bootstrapScreenshotEditor();
