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
  Minus,
  Redo2,
  Square,
  Trash2,
  Type,
  Undo2,
  X,
} from 'lucide-react';
import './index.css';

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

const colors = ['#ef4444', '#f59e0b', '#2563eb', '#22c55e', '#ffffff', '#111827'];
const toolbarButtonClass = 'flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40';
const imageSizeFallback = { width: 1, height: 1 };
const tools: Array<{ id: Tool; label: string; icon: React.ComponentType<{ size?: number }> }> = [
  { id: 'pen', label: 'Brush', icon: Brush },
  { id: 'line', label: 'Line', icon: Minus },
  { id: 'arrow', label: 'Arrow', icon: ArrowUpRight },
  { id: 'ellipse', label: 'Circle', icon: Circle },
  { id: 'rect', label: 'Rectangle', icon: Square },
  { id: 'mosaic', label: 'Mosaic', icon: Grid2X2 },
  { id: 'text', label: 'Text', icon: Type },
];

function ScreenshotEditor() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const sessionId = query.get('session_id') ?? '';
  const baseCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const draftCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  const isFinishedRef = useRef(false);
  const annotationsRef = useRef<Annotation[]>([]);
  const draftRef = useRef<Annotation | null>(null);
  const dragStartRef = useRef<Point | null>(null);
  const [imageLoaded, setImageLoaded] = useState(false);
  const [imageSize, setImageSize] = useState(imageSizeFallback);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [undoStack, setUndoStack] = useState<Annotation[][]>([]);
  const [redoStack, setRedoStack] = useState<Annotation[][]>([]);
  const [tool, setTool] = useState<Tool>('pen');
  const [color, setColor] = useState(colors[0]);
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

  useEffect(() => {
    annotationsRef.current = annotations;
    renderCommitted();
  }, [annotations]);

  useEffect(() => {
    if (!sessionId) {
      setError('Missing editor session.');
      return;
    }

    document.body.tabIndex = -1;
    document.body.focus();
    window.focus();

    void invoke<string>('get_pending_capture_image', { sessionId })
      .then((dataUrl) => {
        const image = new Image();
        image.onload = () => {
          imageRef.current = image;
          setImageSize({ width: image.naturalWidth, height: image.naturalHeight });
          for (const canvas of [baseCanvasRef.current, draftCanvasRef.current]) {
            if (canvas) {
              canvas.width = image.naturalWidth;
              canvas.height = image.naturalHeight;
            }
          }
          setImageLoaded(true);
          requestAnimationFrame(() => renderCommitted());
        };
        image.onerror = () => setError('Failed to load screenshot.');
        image.src = dataUrl;
      })
      .catch((loadError: unknown) => setError(String(loadError)));
  }, [sessionId]);

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
      return;
    }

    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }

    context.clearRect(0, 0, canvas.width, canvas.height);
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    for (const annotation of annotationsRef.current) {
      drawAnnotation(context, annotation);
    }
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

  function handlePointerDown(event: React.PointerEvent<HTMLCanvasElement>) {
    if (!imageRef.current) {
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

  async function handleCancel() {
    if (!sessionId) {
      return;
    }
    if (isFinishedRef.current) {
      return;
    }
    isFinishedRef.current = true;
    await invoke('cancel_capture_edit', { sessionId }).catch((cancelError: unknown) => {
      setError(String(cancelError));
    });
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

    setIsSaving(true);
    setError('');
    isFinishedRef.current = true;
    try {
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
      const pngBase64 = exportCanvas.toDataURL('image/png');
      await invoke('confirm_regular_capture_edit', { sessionId, pngBase64 });
      await getCurrentWindow().close();
    } catch (saveError) {
      isFinishedRef.current = false;
      setError(String(saveError));
      setIsSaving(false);
    }
  }

  return (
    <div className="relative h-screen overflow-hidden bg-black/45 text-on-surface">
      <div
        className="absolute bg-transparent shadow-[0_0_0_2px_rgba(0,102,204,0.95)]"
        style={{
          left: selectionOffset.left,
          top: selectionOffset.top,
          width: displaySize.width,
          height: displaySize.height,
        }}
      >
        <canvas
          ref={baseCanvasRef}
          width={imageSize.width}
          height={imageSize.height}
          className="absolute inset-0 h-full w-full bg-white"
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
        className="absolute z-40 flex max-w-[calc(100vw-16px)] flex-wrap items-center gap-1.5 rounded-lg border border-outline-variant/30 bg-surface-container-lowest/95 p-1.5 shadow-xl backdrop-blur"
        style={{ left: toolbarOffset.left, top: toolbarOffset.top }}
      >
        <div className="flex items-center gap-1">
          {tools.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                type="button"
                title={item.label}
                onClick={() => setTool(item.id)}
                className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md border transition-colors ${
                  tool === item.id
                    ? 'border-primary bg-primary text-white'
                    : 'border-outline-variant/30 bg-surface-container text-on-surface hover:bg-surface-container-high'
                }`}
              >
                <Icon size={18} />
              </button>
            );
          })}
        </div>

        <div className="h-6 w-px shrink-0 bg-outline-variant/40" />

        <div className="flex items-center gap-1">
          {colors.map((item) => (
            <button
              key={item}
              type="button"
              title={item}
              onClick={() => setColor(item)}
              className={`h-6 w-6 shrink-0 rounded-full border-2 ${color === item ? 'border-primary' : 'border-outline-variant/40'}`}
              style={{ backgroundColor: item }}
            />
          ))}
        </div>

        {tool !== 'mosaic' && (
          <label className="flex shrink-0 items-center gap-2 text-xs font-semibold text-on-surface-variant">
            Size
            <input
              type="range"
              min="2"
              max="18"
              value={lineWidth}
              onChange={(event) => setLineWidth(Number(event.target.value))}
              className="w-16 accent-primary"
            />
          </label>
        )}

        {tool === 'mosaic' && (
          <>
            <label className="flex shrink-0 items-center gap-2 text-xs font-semibold text-on-surface-variant">
              Brush
              <input
                type="range"
                min="12"
                max="80"
                value={mosaicWidth}
                onChange={(event) => setMosaicWidth(Number(event.target.value))}
                className="w-16 accent-primary"
              />
            </label>
            <label className="flex shrink-0 items-center gap-2 text-xs font-semibold text-on-surface-variant">
              Block
              <input
                type="range"
                min="6"
                max="32"
                value={mosaicSize}
                onChange={(event) => setMosaicSize(Number(event.target.value))}
                className="w-16 accent-primary"
              />
            </label>
          </>
        )}

        {tool === 'text' && (
          <label className="flex shrink-0 items-center gap-2 text-xs font-semibold text-on-surface-variant">
            Text
            <input
              type="range"
              min="14"
              max="64"
              value={fontSize}
              onChange={(event) => setFontSize(Number(event.target.value))}
              className="w-16 accent-primary"
            />
          </label>
        )}

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
            className="flex h-8 shrink-0 items-center gap-1 rounded-md bg-green-600 px-2 text-xs font-bold text-white transition-colors hover:bg-green-700 disabled:opacity-50"
          >
            <Check size={18} />
            {isSaving ? 'Saving' : 'Done'}
          </button>
        </div>
      </div>

      {error && <div className="fixed bottom-16 left-2 right-2 rounded bg-error/90 px-3 py-2 text-sm text-white">{error}</div>}
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
      const sampleX = Math.min(width - 1, blockX + Math.floor(blockSize / 2));
      const sampleY = Math.min(height - 1, blockY + Math.floor(blockSize / 2));
      const sampleIndex = (sampleY * width + sampleX) * 4;
      const red = data.data[sampleIndex];
      const green = data.data[sampleIndex + 1];
      const blue = data.data[sampleIndex + 2];
      const alpha = data.data[sampleIndex + 3];

      for (let py = blockY; py < Math.min(blockY + blockSize, height); py += 1) {
        for (let px = blockX; px < Math.min(blockX + blockSize, width); px += 1) {
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

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ScreenshotEditor />
  </React.StrictMode>,
);
