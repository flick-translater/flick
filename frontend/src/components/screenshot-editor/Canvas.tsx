import type React from 'react';
import type { Point, Rect, TextDraft } from './types';

type ResizeHandleView = {
  id: string;
  x: number;
  y: number;
};

type ScreenshotEditorCanvasProps = {
  selectionOffset: { left: number; top: number };
  displaySize: { width: number; height: number };
  editorVisible: boolean;
  screenshotDataUrl: string;
  baseCanvasRef: React.RefObject<HTMLCanvasElement | null>;
  draftCanvasRef: React.RefObject<HTMLCanvasElement | null>;
  imageSize: { width: number; height: number };
  onPointerDown: (event: React.PointerEvent<HTMLCanvasElement>) => void;
  onPointerMove: (event: React.PointerEvent<HTMLCanvasElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLCanvasElement>) => void;
  onPointerCancel: () => void;
  onPointerLeave: () => void;
  canvasCursor: string;
  selectedControlRect: Rect | null;
  canvasRectToCss: (rect: Rect) => Rect;
  resizeHandles: (rect: Rect) => ResizeHandleView[];
  textDraft: TextDraft | null;
  textAreaRef: React.RefObject<HTMLTextAreaElement | null>;
  setTextDraft: (draft: TextDraft | null) => void;
  commitTextDraft: () => void;
  color: string;
  fontSize: number;
  viewportImageOffsetY?: number;
  showAnnotationLayer?: boolean;
  framed?: boolean;
  insetFrame?: boolean;
};

export function ScreenshotEditorCanvas({
  selectionOffset,
  displaySize,
  editorVisible,
  screenshotDataUrl,
  baseCanvasRef,
  draftCanvasRef,
  imageSize,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onPointerCancel,
  onPointerLeave,
  canvasCursor,
  selectedControlRect,
  canvasRectToCss,
  resizeHandles,
  textDraft,
  textAreaRef,
  setTextDraft,
  commitTextDraft,
  color,
  fontSize,
  viewportImageOffsetY = 0,
  showAnnotationLayer = true,
  framed = true,
  insetFrame = false,
}: ScreenshotEditorCanvasProps) {
  const selectedControlCssRect = selectedControlRect ? canvasRectToCss(selectedControlRect) : null;
  const frameShadow = insetFrame
    ? 'inset 0 0 0 2px rgba(0,102,204,0.95)'
    : '0 0 0 2px rgba(0,102,204,0.95)';

  return (
    <div
      className="absolute overflow-hidden bg-transparent"
      style={{
        left: selectionOffset.left,
        top: selectionOffset.top,
        width: displaySize.width,
        height: displaySize.height,
        opacity: editorVisible ? 1 : 0,
        pointerEvents: showAnnotationLayer ? 'auto' : 'none',
      }}
    >
      {screenshotDataUrl && showAnnotationLayer && (
        <img
          src={screenshotDataUrl}
          alt=""
          className="pointer-events-none absolute inset-0 h-full w-full select-none"
          draggable={false}
        />
      )}
      {screenshotDataUrl && !showAnnotationLayer && (
        <img
          src={screenshotDataUrl}
          alt=""
          draggable={false}
          className="pointer-events-none absolute inset-0 h-full w-full select-none"
          style={{
            transform: `translateY(${-viewportImageOffsetY}px)`,
          }}
        />
      )}
      <canvas
        ref={baseCanvasRef}
        width={imageSize.width}
        height={imageSize.height}
        className="absolute inset-0 h-full w-full"
        style={{
          opacity: showAnnotationLayer ? 1 : 0,
          pointerEvents: showAnnotationLayer ? 'auto' : 'none',
        }}
      />
      <canvas
        ref={draftCanvasRef}
        width={imageSize.width}
        height={imageSize.height}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onPointerLeave={onPointerLeave}
        className="absolute inset-0 h-full w-full"
        style={{
          cursor: canvasCursor,
          opacity: showAnnotationLayer ? 1 : 0,
          pointerEvents: showAnnotationLayer ? 'auto' : 'none',
        }}
      />
      {framed && (
        <div
          className="pointer-events-none absolute inset-0 z-10"
          style={{ boxShadow: frameShadow }}
        />
      )}
      {showAnnotationLayer && selectedControlCssRect && (
        <div
          className="pointer-events-none absolute z-20 border border-primary"
          style={{
            left: selectedControlCssRect.x,
            top: selectedControlCssRect.y,
            width: selectedControlCssRect.width,
            height: selectedControlCssRect.height,
            boxShadow: '0 0 0 1px rgba(255,255,255,0.9)',
          }}
        >
          {resizeHandles(selectedControlCssRect).map((handle) => (
            <span
              key={handle.id}
              className="absolute h-2 w-2 rounded-sm border border-primary bg-white shadow-sm"
              style={{
                left: handle.x,
                top: handle.y,
                transform: 'translate(-50%, -50%)',
              }}
            />
          ))}
        </div>
      )}
      {showAnnotationLayer && textDraft && (
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
  );
}
