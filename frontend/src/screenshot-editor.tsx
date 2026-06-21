import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  ArrowUpRight,
  Brush,
  Check,
  Circle,
  Grid2X2,
  Redo2,
  Slash,
  Square,
  Trash2,
  Type,
  Undo2,
  X,
} from 'lucide-react';
import './index.css';
import type { AppSettings } from './types';

type Point = { x: number; y: number };
type Rect = { x: number; y: number; width: number; height: number };
type Tool = 'pen' | 'line' | 'arrow' | 'ellipse' | 'rect' | 'mosaic' | 'text';

type Annotation =
  | { kind: 'pen'; points: Point[]; color: string; width: number }
  | { kind: 'line'; from: Point; to: Point; color: string; width: number }
  | { kind: 'arrow'; from: Point; to: Point; color: string; width: number }
  | { kind: 'ellipse'; rect: Rect; color: string; width: number }
  | { kind: 'rect'; rect: Rect; color: string; width: number }
  | { kind: 'mosaic'; points: Point[]; width: number; blockSize: number }
  | { kind: 'text'; position: Point; text: string; color: string; fontSize: number };

type TextDraft = {
  position: Point;
  cssPosition: Point;
  value: string;
};

const defaultEditorColor = '#ef4444';
const toolbarButtonClass = 'flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40';
const toolOptionPanelClass = 'absolute left-0 top-[calc(100%+8px)] z-50 w-40 rounded-lg border border-outline-variant/40 bg-surface-container-lowest p-3 shadow-2xl';
const imageSizeFallback = { width: 1, height: 1 };
const tools: Array<{ id: Tool; label: string; icon: React.ComponentType<{ size?: number }> }> = [
  { id: 'pen', label: 'Brush', icon: Brush },
  { id: 'line', label: 'Line', icon: Slash },
  { id: 'arrow', label: 'Arrow', icon: ArrowUpRight },
  { id: 'ellipse', label: 'Circle', icon: Circle },
  { id: 'rect', label: 'Rectangle', icon: Square },
  { id: 'mosaic', label: 'Mosaic', icon: Grid2X2 },
  { id: 'text', label: 'Text', icon: Type },
];

function editorLog(_step: string) {}

function ScreenshotEditor() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const initialColor = useMemo(() => {
    const rawColor = query.get('color') ?? '';
    const normalized = rawColor.startsWith('#') ? rawColor : `#${rawColor}`;
    return isHexColor(normalized) ? normalized.toLowerCase() : defaultEditorColor;
  }, [query]);
  const sessionId = query.get('session_id') ?? '';
  const isPreload = query.get('preload') === '1';
  const baseCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const draftCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  const isFinishedRef = useRef(false);
  const isClosingRef = useRef(false);
  const readyNotifiedRef = useRef(false);
  const annotationsRef = useRef<Annotation[]>([]);
  const draftRef = useRef<Annotation | null>(null);
  const dragStartRef = useRef<Point | null>(null);
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
  const [error, setError] = useState('');
  const [isSaving, setIsSaving] = useState(false);
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
  const [toolbarPosition, setToolbarPosition] = useState(toolbarOffset);

  useEffect(() => {
    annotationsRef.current = annotations;
    renderCommitted();
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

    editorLog('frontend load: get_pending_capture_image start');
    void invoke<string>('get_pending_capture_image', { sessionId })
      .then((dataUrl) => {
        editorLog(`frontend load: data url received length=${dataUrl.length}`);
        setScreenshotDataUrl(dataUrl);
        const image = new Image();
        image.onload = () => {
          editorLog(`frontend load: image decoded natural=${image.naturalWidth}x${image.naturalHeight}`);
          imageRef.current = image;
          setImageSize({ width: image.naturalWidth, height: image.naturalHeight });
          for (const canvas of [baseCanvasRef.current, draftCanvasRef.current]) {
            if (canvas) {
              canvas.width = image.naturalWidth;
              canvas.height = image.naturalHeight;
            }
          }
          setImageLoaded(true);
          editorLog('frontend load: canvas sized');
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
  }, [isPreload, sessionId]);

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

  useEffect(() => {
    const appWindow = getCurrentWindow();
    const unlistenPromise = appWindow.onCloseRequested(async (event) => {
      if (isFinishedRef.current || !sessionId) {
        return;
      }
      event.preventDefault();
      isClosingRef.current = true;
      isFinishedRef.current = true;
      await invoke('cancel_capture_edit', { sessionId }).catch(() => undefined);
      await appWindow.close();
    });

    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [sessionId]);

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
    commitAnnotations([...annotationsRef.current, annotation]);
  }, [commitAnnotations]);

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
    setTool((currentTool) => currentTool === nextTool ? null : nextTool);
  }

  function handlePointerDown(event: React.PointerEvent<HTMLCanvasElement>) {
    if (!imageRef.current) {
      return;
    }
    setColorPickerOpen(false);
    if (!tool) {
      return;
    }

    const point = getCanvasPoint(event);
    if (tool === 'text') {
      setTextDraft({ position: point, cssPosition: getCssPoint(event), value: '' });
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
    if (!isDragging || !draftRef.current || !dragStartRef.current) {
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
    renderDraft(null);
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

  async function handleCancel() {
    if (!sessionId) {
      return;
    }
    if (isFinishedRef.current) {
      return;
    }
    editorLog('cancel click: start');
    isClosingRef.current = true;
    isFinishedRef.current = true;
    await invoke('cancel_capture_edit', { sessionId }).catch((cancelError: unknown) => {
      editorLog(`cancel click: failed ${String(cancelError)}`);
      setError(String(cancelError));
    });
    editorLog('cancel click: close window');
    await getCurrentWindow().close();
  }

  async function handleConfirm() {
    const image = imageRef.current;
    if (!image || !sessionId) {
      return;
    }
    if (isFinishedRef.current) {
      return;
    }

    editorLog('confirm click: start');
    setIsSaving(true);
    setError('');
    isFinishedRef.current = true;
    try {
      editorLog('confirm click: create export canvas');
      const exportCanvas = document.createElement('canvas');
      exportCanvas.width = image.naturalWidth;
      exportCanvas.height = image.naturalHeight;
      const context = exportCanvas.getContext('2d');
      if (!context) {
        throw new Error('Failed to create export canvas.');
      }
      editorLog('confirm click: draw base image');
      context.drawImage(image, 0, 0);
      editorLog('confirm click: draw annotations');
      for (const annotation of annotationsRef.current) {
        drawAnnotation(context, annotation);
      }
      editorLog('confirm click: encode png data url start');
      const pngBase64 = exportCanvas.toDataURL('image/png');
      editorLog('confirm click: encode png data url complete');
      editorLog('confirm click: invoke confirm_regular_capture_edit start');
      await invoke('confirm_regular_capture_edit', { sessionId, pngBase64 });
      editorLog('confirm click: invoke confirm_regular_capture_edit complete');
      editorLog('confirm click: close window');
      await getCurrentWindow().close();
    } catch (saveError) {
      isFinishedRef.current = false;
      editorLog(`confirm click: failed ${String(saveError)}`);
      setError(String(saveError));
      setIsSaving(false);
    }
  }

  return (
    <div
      className="relative h-screen overflow-hidden text-on-surface"
      style={{ backgroundColor: 'transparent' }}
    >
      {!isPreload && (
        <>
      <div
        className="absolute bg-transparent shadow-[0_0_0_2px_rgba(0,102,204,0.95)]"
        style={{
          left: selectionOffset.left,
          top: selectionOffset.top,
          width: displaySize.width,
          height: displaySize.height,
          opacity: editorVisible ? 1 : 0,
        }}
      >
        {screenshotDataUrl && (
          <img
            src={screenshotDataUrl}
            alt=""
            className="pointer-events-none absolute inset-0 h-full w-full select-none"
            draggable={false}
          />
        )}
        <canvas
          ref={baseCanvasRef}
          width={imageSize.width}
          height={imageSize.height}
          className="absolute inset-0 h-full w-full"
        />
        <canvas
          ref={draftCanvasRef}
          width={imageSize.width}
          height={imageSize.height}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerCancel={() => {
            draftRef.current = null;
            dragStartRef.current = null;
            setIsDragging(false);
            renderDraft(null);
          }}
          className="absolute inset-0 h-full w-full cursor-crosshair"
        />
        {textDraft && (
          <textarea
            ref={textAreaRef}
            value={textDraft.value}
            onChange={(event) => setTextDraft({ ...textDraft, value: event.target.value })}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                setTextDraft(null);
              } else if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                event.stopPropagation();
                commitTextDraft();
              }
            }}
            onPointerDown={(event) => event.stopPropagation()}
            className="absolute z-30 min-h-10 min-w-32 resize rounded border border-primary bg-white/95 px-2 py-1 font-bold text-on-surface shadow-lg outline-none"
            style={{
              left: textDraft.cssPosition.x,
              top: textDraft.cssPosition.y,
              color,
              fontSize,
              lineHeight: 1.25,
            }}
          />
        )}
      </div>

      <div
        ref={toolbarRef}
        className="absolute z-40 flex max-w-[calc(100vw-16px)] items-center gap-1.5 rounded-lg border border-outline-variant/30 bg-surface-container-lowest/95 p-1.5 shadow-xl backdrop-blur"
        style={{
          left: toolbarPosition.left,
          top: toolbarPosition.top,
          opacity: editorVisible ? 1 : 0,
          pointerEvents: editorVisible ? 'auto' : 'none',
        }}
      >
        <div className="flex items-center gap-1">
          {tools.map((item) => {
            const Icon = item.icon;
            const isActive = tool === item.id;
            return (
              <div key={item.id} className="relative shrink-0">
                <button
                  type="button"
                  title={item.label}
                  onClick={() => handleToolClick(item.id)}
                  className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md border transition-colors ${
                    isActive
                      ? 'border-primary bg-primary text-white'
                      : 'border-outline-variant/30 bg-surface-container text-on-surface hover:bg-surface-container-high'
                  }`}
                >
                  <Icon size={18} />
                </button>
                {isActive && item.id !== 'mosaic' && item.id !== 'text' && (
                  <ToolOptionPanel label="Size">
                    <input
                      type="range"
                      min="2"
                      max="18"
                      value={lineWidth}
                      onChange={(event) => setLineWidth(Number(event.target.value))}
                      className="w-full accent-primary"
                    />
                  </ToolOptionPanel>
                )}
                {isActive && item.id === 'mosaic' && (
                  <ToolOptionPanel label="Brush">
                    <input
                      type="range"
                      min="12"
                      max="80"
                      value={mosaicWidth}
                      onChange={(event) => setMosaicWidth(Number(event.target.value))}
                      className="w-full accent-primary"
                    />
                    <div className="mt-3 text-xs font-semibold text-on-surface-variant">Block</div>
                    <input
                      type="range"
                      min="6"
                      max="32"
                      value={mosaicSize}
                      onChange={(event) => setMosaicSize(Number(event.target.value))}
                      className="mt-1 w-full accent-primary"
                    />
                  </ToolOptionPanel>
                )}
                {isActive && item.id === 'text' && (
                  <ToolOptionPanel label="Text">
                    <input
                      type="range"
                      min="14"
                      max="64"
                      value={fontSize}
                      onChange={(event) => setFontSize(Number(event.target.value))}
                      className="w-full accent-primary"
                    />
                  </ToolOptionPanel>
                )}
              </div>
            );
          })}
        </div>

        <div className="h-6 w-px shrink-0 bg-outline-variant/40" />

        <div className="relative shrink-0">
          <button
            type="button"
            title="Color"
            onClick={() => setColorPickerOpen((open) => !open)}
            className="h-8 w-8 rounded-md border-2 border-outline-variant/60 shadow-inner"
            style={{ backgroundColor: color }}
          />
          {colorPickerOpen && (
            <div
              className="absolute left-0 top-[calc(100%+8px)] z-50 w-56 rounded-lg border border-outline-variant/40 bg-surface-container-lowest p-3 shadow-2xl"
              onPointerDown={(event) => event.stopPropagation()}
            >
              <div
                className="relative h-32 w-full cursor-crosshair overflow-hidden rounded-md border border-outline-variant/30"
                style={{
                  backgroundColor: hsvToHex({ h: hexToHsv(color).h, s: 1, v: 1 }),
                }}
                onPointerDown={(event) => {
                  event.currentTarget.setPointerCapture(event.pointerId);
                  handleColorSquarePointer(event);
                }}
                onPointerMove={(event) => {
                  if (event.buttons === 1) {
                    handleColorSquarePointer(event);
                  }
                }}
              >
                <div className="absolute inset-0 bg-gradient-to-r from-white to-transparent" />
                <div className="absolute inset-0 bg-gradient-to-b from-transparent to-black" />
                <div
                  className="absolute h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow"
                  style={{
                    left: `${hexToHsv(color).s * 100}%`,
                    top: `${(1 - hexToHsv(color).v) * 100}%`,
                  }}
                />
              </div>
              <input
                type="range"
                min="0"
                max="360"
                value={Math.round(hexToHsv(color).h)}
                onChange={(event) => {
                  const hsv = hexToHsv(color);
                  handleColorChange(hsvToHex({ ...hsv, h: Number(event.target.value) }));
                }}
                className="mt-3 h-2 w-full accent-primary"
                style={{
                  background: 'linear-gradient(to right, #ff0000, #ffff00, #00ff00, #00ffff, #0000ff, #ff00ff, #ff0000)',
                }}
              />
              <div className="mt-3 flex items-center gap-2">
                <div className="h-8 w-8 rounded-md border border-outline-variant/40" style={{ backgroundColor: color }} />
                <input
                  value={hexInput}
                  onChange={(event) => setHexInput(event.target.value)}
                  onBlur={handleHexInputCommit}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      handleHexInputCommit();
                    }
                  }}
                  className="h-8 min-w-0 flex-1 rounded-md border border-outline-variant/40 bg-surface-container px-2 text-xs font-bold uppercase outline-none focus:border-primary"
                />
              </div>
            </div>
          )}
        </div>

        <div className="flex items-center gap-1">
          <button type="button" title="Undo" onClick={undo} disabled={undoStack.length === 0} className={toolbarButtonClass}>
            <Undo2 size={18} />
          </button>
          <button type="button" title="Redo" onClick={redo} disabled={redoStack.length === 0} className={toolbarButtonClass}>
            <Redo2 size={18} />
          </button>
          <button type="button" title="Clear" onClick={() => commitAnnotations([])} className={toolbarButtonClass}>
            <Trash2 size={18} />
          </button>
          <button
            type="button"
            title="Cancel"
            onClick={() => void handleCancel()}
            className="flex h-8 w-8 items-center justify-center rounded-md border border-red-600 bg-red-600 text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <X size={18} />
          </button>
          <button
            type="button"
            title="Confirm"
            disabled={isSaving || !imageLoaded}
            onClick={() => void handleConfirm()}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-green-600 text-white transition-colors hover:bg-green-700 disabled:opacity-50"
          >
            <Check size={18} />
          </button>
        </div>
      </div>

      {error && <div className="fixed bottom-16 left-2 right-2 rounded bg-error/90 px-3 py-2 text-sm text-white">{error}</div>}
        </>
      )}
    </div>
  );
}

function ToolOptionPanel({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className={toolOptionPanelClass} onPointerDown={(event) => event.stopPropagation()}>
      <div className="mb-1 text-xs font-semibold text-on-surface-variant">{label}</div>
      {children}
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
  }

  context.restore();
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

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<ScreenshotEditor />);
