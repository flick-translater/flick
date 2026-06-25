import type React from 'react';
import {
  ArrowUpRight,
  ArrowUpDown,
  Brush,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Circle,
  Edit3,
  Pause,
  Play,
  Redo2,
  Smile,
  Slash,
  Square,
  Trash2,
  Type,
  Undo2,
  Video,
  X,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { Tool } from './types';

const toolbarButtonClass = 'flex h-8 w-8 items-center justify-center rounded-md border border-outline-variant/30 bg-surface-container text-on-surface transition-colors hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-40';
const toolOptionPanelClass = 'absolute left-0 z-50 w-40 rounded-lg border border-outline-variant/40 bg-surface-container-lowest p-3 shadow-2xl';
const toolbarSeparatorClass = 'h-6 w-px shrink-0 bg-outline-variant/40';
const toolbarTooltipClass = 'pointer-events-none absolute left-1/2 z-[80] -translate-x-1/2 whitespace-nowrap rounded-md bg-surface-container-highest px-2 py-1 text-xs font-semibold text-on-surface opacity-0 shadow-lg ring-1 ring-outline-variant/30 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100';

const tools: Array<{ id: Tool; labelKey: string; icon: React.ComponentType<{ size?: number; active?: boolean }> }> = [
  { id: 'pen', labelKey: 'screenshotEditor.tools.brush', icon: Brush },
  { id: 'line', labelKey: 'screenshotEditor.tools.line', icon: Slash },
  { id: 'arrow', labelKey: 'screenshotEditor.tools.arrow', icon: ArrowUpRight },
  { id: 'ellipse', labelKey: 'screenshotEditor.tools.circle', icon: Circle },
  { id: 'rect', labelKey: 'screenshotEditor.tools.rectangle', icon: Square },
  { id: 'mosaic', labelKey: 'screenshotEditor.tools.mosaic', icon: CheckerboardIcon },
  { id: 'text', labelKey: 'screenshotEditor.tools.text', icon: Type },
  { id: 'emoji', labelKey: 'screenshotEditor.tools.emoji', icon: Smile },
];

type ScreenshotEditorToolbarProps = {
  toolbarRef: React.RefObject<HTMLDivElement | null>;
  toolbarPosition: { left: number; top: number };
  editorVisible: boolean;
  tool: Tool | null;
  onToolClick: (tool: Tool) => void;
  popupPositionClass: string;
  lineWidth: number;
  setLineWidth: (width: number) => void;
  mosaicWidth: number;
  setMosaicWidth: (width: number) => void;
  mosaicSize: number;
  setMosaicSize: (size: number) => void;
  fontSize: number;
  setFontSize: (size: number) => void;
  emojiPickerOpen: boolean;
  visibleEmojiChoices: string[];
  onEmojiChoice: (emoji: string) => void;
  emojiPage: number;
  emojiPageCount: number;
  emojiCount: number;
  setEmojiPage: React.Dispatch<React.SetStateAction<number>>;
  color: string;
  colorPickerOpen: boolean;
  setColorPickerOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setEmojiPickerOpen: React.Dispatch<React.SetStateAction<boolean>>;
  handleColorSquarePointer: (event: React.PointerEvent<HTMLDivElement>) => void;
  handleColorChange: (color: string) => void;
  hexInput: string;
  setHexInput: (value: string) => void;
  handleHexInputCommit: () => void;
  hexToHsv: (hex: string) => { h: number; s: number; v: number };
  hsvToHex: (hsv: { h: number; s: number; v: number }) => string;
  undo: () => void;
  redo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  clearAnnotations: () => void;
  onLongCapture: () => void;
  onGifRecording: () => void;
  onCancel: () => void;
  onConfirm: () => void;
  isSaving: boolean;
  imageLoaded: boolean;
  embedded?: boolean;
  showLongCapture?: boolean;
  useNativeTooltip?: boolean;
};

export function ScreenshotEditorToolbar({
  toolbarRef,
  toolbarPosition,
  editorVisible,
  tool,
  onToolClick,
  popupPositionClass,
  lineWidth,
  setLineWidth,
  mosaicWidth,
  setMosaicWidth,
  mosaicSize,
  setMosaicSize,
  fontSize,
  setFontSize,
  emojiPickerOpen,
  visibleEmojiChoices,
  onEmojiChoice,
  emojiPage,
  emojiPageCount,
  emojiCount,
  setEmojiPage,
  color,
  colorPickerOpen,
  setColorPickerOpen,
  setEmojiPickerOpen,
  handleColorSquarePointer,
  handleColorChange,
  hexInput,
  setHexInput,
  handleHexInputCommit,
  hexToHsv,
  hsvToHex,
  undo,
  redo,
  canUndo,
  canRedo,
  clearAnnotations,
  onLongCapture,
  onGifRecording,
  onCancel,
  onConfirm,
  isSaving,
  imageLoaded,
  embedded = false,
  showLongCapture = true,
  useNativeTooltip = false,
}: ScreenshotEditorToolbarProps) {
  const { t } = useTranslation();
  const hsvColor = hexToHsv(color);
  const toolbarClassName = embedded
    ? 'relative z-40 flex max-w-[calc(100vw-16px)] items-center gap-1.5 bg-transparent p-0'
    : 'absolute z-40 flex max-w-[calc(100vw-16px)] items-center gap-1.5 rounded-lg border border-outline-variant/30 bg-surface-container-lowest/95 p-1.5 shadow-xl backdrop-blur';
  const tooltipPositionClass = embedded ? 'top-full mt-2' : 'bottom-full mb-2';

  return (
    <div
      ref={toolbarRef}
      className={toolbarClassName}
      style={embedded
        ? {
            opacity: editorVisible ? 1 : 0,
            pointerEvents: editorVisible ? 'auto' : 'none',
          }
        : {
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
          const label = t(item.labelKey);
          return (
            <div key={item.id} className="group relative shrink-0" title={useNativeTooltip ? label : undefined}>
              <button
                type="button"
                aria-label={label}
                onClick={() => onToolClick(item.id)}
                className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md border transition-colors ${
                  isActive
                    ? 'border-primary bg-primary text-white'
                    : 'border-outline-variant/30 bg-surface-container text-on-surface hover:bg-surface-container-high'
                }`}
              >
                <Icon size={18} active={isActive} />
              </button>
              {!useNativeTooltip && <ToolbarTooltip label={label} positionClass={tooltipPositionClass} />}
              {isActive && item.id !== 'mosaic' && item.id !== 'text' && item.id !== 'emoji' && (
                <ToolOptionPanel label={t('screenshotEditor.options.size')} positionClass={popupPositionClass}>
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
                <ToolOptionPanel label={t('screenshotEditor.options.brush')} positionClass={popupPositionClass}>
                  <input
                    type="range"
                    min="12"
                    max="80"
                    value={mosaicWidth}
                    onChange={(event) => setMosaicWidth(Number(event.target.value))}
                    className="w-full accent-primary"
                  />
                  <div className="mt-3 text-xs font-semibold text-on-surface-variant">{t('screenshotEditor.options.block')}</div>
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
                <ToolOptionPanel label={t('screenshotEditor.options.text')} positionClass={popupPositionClass}>
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
              {isActive && item.id === 'emoji' && emojiPickerOpen && (
                <div
                  className={`absolute left-0 ${popupPositionClass} z-50 w-72 rounded-lg border border-outline-variant/40 bg-surface-container-lowest p-2 shadow-2xl`}
                  onPointerDown={(event) => event.stopPropagation()}
                >
                  <div className="grid grid-cols-8 gap-1">
                    {visibleEmojiChoices.map((emoji) => (
                      <button
                        key={emoji}
                        type="button"
                        title={emoji}
                        aria-label={emoji}
                        onClick={() => onEmojiChoice(emoji)}
                        className="flex h-8 w-8 items-center justify-center rounded-md text-xl leading-none hover:bg-surface-container-high"
                      >
                        {emoji}
                      </button>
                    ))}
                  </div>
                  <div className="mt-2 flex items-center justify-between border-t border-outline-variant/30 pt-2">
                    <TooltipButton
                      label={t('screenshotEditor.actions.previous')}
                      positionClass="bottom-full mb-2"
                      useNativeTooltip={useNativeTooltip}
                    >
                      <button
                        type="button"
                        aria-label={t('screenshotEditor.actions.previous')}
                        disabled={emojiPage === 0}
                        onClick={() => setEmojiPage((page) => Math.max(page - 1, 0))}
                        className={toolbarButtonClass}
                      >
                        <ChevronLeft size={16} />
                      </button>
                    </TooltipButton>
                    <div className="px-2 text-xs font-semibold text-on-surface-variant">
                      {emojiPage + 1}/{emojiPageCount} · {emojiCount}
                    </div>
                    <TooltipButton
                      label={t('screenshotEditor.actions.next')}
                      positionClass="bottom-full mb-2"
                      useNativeTooltip={useNativeTooltip}
                    >
                      <button
                        type="button"
                        aria-label={t('screenshotEditor.actions.next')}
                        disabled={emojiPage >= emojiPageCount - 1}
                        onClick={() => setEmojiPage((page) => Math.min(page + 1, emojiPageCount - 1))}
                        className={toolbarButtonClass}
                      >
                        <ChevronRight size={16} />
                      </button>
                    </TooltipButton>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className={toolbarSeparatorClass} />

      <div className="group relative shrink-0" title={useNativeTooltip ? t('screenshotEditor.actions.color') : undefined}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.color')}
          onClick={() => {
            setEmojiPickerOpen(false);
            setColorPickerOpen((open) => !open);
          }}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-outline-variant/30 shadow-inner"
          style={{ backgroundColor: color }}
        />
        {!useNativeTooltip && <ToolbarTooltip label={t('screenshotEditor.actions.color')} positionClass={tooltipPositionClass} />}
        {colorPickerOpen && (
          <div
            className={`absolute left-0 ${popupPositionClass} z-50 w-56 rounded-lg border border-outline-variant/40 bg-surface-container-lowest p-3 shadow-2xl`}
            onPointerDown={(event) => event.stopPropagation()}
          >
            <div
              className="relative h-32 w-full cursor-crosshair overflow-hidden rounded-md border border-outline-variant/30"
              style={{
                backgroundColor: hsvToHex({ h: hsvColor.h, s: 1, v: 1 }),
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
                  left: `${hsvColor.s * 100}%`,
                  top: `${(1 - hsvColor.v) * 100}%`,
                }}
              />
            </div>
            <input
              type="range"
              min="0"
              max="360"
              value={Math.round(hsvColor.h)}
              onChange={(event) => {
                handleColorChange(hsvToHex({ ...hsvColor, h: Number(event.target.value) }));
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

      {showLongCapture && (
        <>
          <div className={toolbarSeparatorClass} />

          <div className="flex items-center gap-1">
            <TooltipButton
              label={t('screenshotEditor.actions.longScreenshot')}
              positionClass={tooltipPositionClass}
              useNativeTooltip={useNativeTooltip}
            >
              <button
                type="button"
                aria-label={t('screenshotEditor.actions.longScreenshot')}
                disabled={isSaving || !imageLoaded}
                onClick={onLongCapture}
                className={toolbarButtonClass}
              >
                <LongScreenshotIcon />
              </button>
            </TooltipButton>
            <TooltipButton
              label={t('screenshotEditor.actions.recordGif')}
              positionClass={tooltipPositionClass}
              useNativeTooltip={useNativeTooltip}
            >
              <button
                type="button"
                aria-label={t('screenshotEditor.actions.recordGif')}
                disabled={isSaving || !imageLoaded}
                onClick={onGifRecording}
                className={toolbarButtonClass}
              >
                <Video size={18} />
              </button>
            </TooltipButton>
          </div>
        </>
      )}

      <div className={toolbarSeparatorClass} />

      <div className="flex items-center gap-1">
        <TooltipButton label={t('screenshotEditor.actions.undo')} positionClass={tooltipPositionClass} useNativeTooltip={useNativeTooltip}>
          <button type="button" aria-label={t('screenshotEditor.actions.undo')} onClick={undo} disabled={!canUndo} className={toolbarButtonClass}>
            <Undo2 size={18} />
          </button>
        </TooltipButton>
        <TooltipButton label={t('screenshotEditor.actions.redo')} positionClass={tooltipPositionClass} useNativeTooltip={useNativeTooltip}>
          <button type="button" aria-label={t('screenshotEditor.actions.redo')} onClick={redo} disabled={!canRedo} className={toolbarButtonClass}>
            <Redo2 size={18} />
          </button>
        </TooltipButton>
      </div>

      <div className={toolbarSeparatorClass} />

      <div className="flex items-center gap-1">
        <TooltipButton label={t('screenshotEditor.actions.clear')} positionClass={tooltipPositionClass} useNativeTooltip={useNativeTooltip}>
          <button type="button" aria-label={t('screenshotEditor.actions.clear')} onClick={clearAnnotations} className={toolbarButtonClass}>
            <Trash2 size={18} />
          </button>
        </TooltipButton>
        <TooltipButton label={t('screenshotEditor.actions.cancel')} positionClass={tooltipPositionClass} useNativeTooltip={useNativeTooltip}>
          <button
            type="button"
            aria-label={t('screenshotEditor.actions.cancel')}
            onClick={onCancel}
            className="flex h-8 w-8 items-center justify-center rounded-md border border-red-600 bg-red-600 text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <X size={18} />
          </button>
        </TooltipButton>
        <TooltipButton label={t('screenshotEditor.actions.confirm')} positionClass={tooltipPositionClass} useNativeTooltip={useNativeTooltip}>
          <button
            type="button"
            aria-label={t('screenshotEditor.actions.confirm')}
            disabled={isSaving || !imageLoaded}
            onClick={onConfirm}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-green-600 text-white transition-colors hover:bg-green-700 disabled:opacity-50"
          >
            <Check size={18} />
          </button>
        </TooltipButton>
      </div>
    </div>
  );
}

type RecordingControlsToolbarProps = {
  toolbarRef: React.RefObject<HTMLDivElement | null>;
  toolbarPosition: { left: number; top: number };
  editorVisible: boolean;
  status: 'idle' | 'recording' | 'paused' | 'saving';
  format: 'gif' | 'video';
  onFormatChange: (format: 'gif' | 'video') => void;
  onStart: () => void;
  onPause: () => void;
  onFinish: () => void;
  onCancel: () => void;
  useNativeTooltip?: boolean;
};

export function RecordingControlsToolbar({
  toolbarRef,
  toolbarPosition,
  editorVisible,
  status,
  format,
  onFormatChange,
  onStart,
  onPause,
  onFinish,
  onCancel,
  useNativeTooltip = false,
}: RecordingControlsToolbarProps) {
  const { t } = useTranslation();
  const isSaving = status === 'saving';
  const isRecording = status === 'recording';
  const isPaused = status === 'paused';

  return (
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
      <TooltipButton label={isPaused ? t('screenshotEditor.actions.resumeRecording') : t('screenshotEditor.actions.startRecording')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={isPaused ? t('screenshotEditor.actions.resumeRecording') : t('screenshotEditor.actions.startRecording')}
          disabled={isSaving || isRecording}
          onClick={onStart}
          className={toolbarButtonClass}
        >
          <Play size={18} />
        </button>
      </TooltipButton>
      <TooltipButton label={t('screenshotEditor.actions.pauseRecording')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.pauseRecording')}
          disabled={isSaving || !isRecording}
          onClick={onPause}
          className={toolbarButtonClass}
        >
          <Pause size={18} />
        </button>
      </TooltipButton>
      <div className="mx-1 h-6 w-px shrink-0 bg-outline-variant/40" />
      <div className="inline-flex shrink-0 rounded-md border border-outline-variant/30 bg-surface-container p-0.5">
        {(['gif', 'video'] as const).map((item) => (
          <button
            key={item}
            type="button"
            disabled={status !== 'idle'}
            onClick={() => onFormatChange(item)}
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
      <TooltipButton label={t('screenshotEditor.actions.finishRecording')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.finishRecording')}
          disabled={isSaving || status === 'idle'}
          onClick={onFinish}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-green-600 text-white transition-colors hover:bg-green-700 disabled:opacity-50"
        >
          <Square size={14} fill="currentColor" />
        </button>
      </TooltipButton>
      <TooltipButton label={t('screenshotEditor.actions.cancel')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.cancel')}
          disabled={isSaving}
          onClick={onCancel}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-red-600 bg-red-600 text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <X size={18} />
        </button>
      </TooltipButton>
    </div>
  );
}

type LongScreenshotToolbarProps = {
  toolbarRef: React.RefObject<HTMLDivElement | null>;
  toolbarPosition: { left: number; top: number };
  editorVisible: boolean;
  onEdit: () => void;
  onScrollStart: (direction: 'up' | 'down') => void;
  onScrollStop: () => void;
  onCancel: () => void;
  onConfirm: () => void;
  isSaving: boolean;
  imageLoaded: boolean;
  useNativeTooltip?: boolean;
};

export function LongScreenshotToolbar({
  toolbarRef,
  toolbarPosition,
  editorVisible,
  onEdit,
  onScrollStart,
  onScrollStop,
  onCancel,
  onConfirm,
  isSaving,
  imageLoaded,
  useNativeTooltip = false,
}: LongScreenshotToolbarProps) {
  const { t } = useTranslation();
  return (
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
      <TooltipButton label={t('screenshotEditor.actions.edit')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.edit')}
          disabled={isSaving || !imageLoaded}
          onClick={onEdit}
          className={toolbarButtonClass}
        >
          <Edit3 size={18} />
        </button>
      </TooltipButton>
      <TooltipButton label={t('screenshotEditor.actions.scrollUp')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.scrollUp')}
          disabled={isSaving || !imageLoaded}
          onPointerDown={(event) => {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            onScrollStart('up');
          }}
          onPointerUp={onScrollStop}
          onPointerCancel={onScrollStop}
          className={toolbarButtonClass}
        >
          <ChevronUp size={18} />
        </button>
      </TooltipButton>
      <TooltipButton label={t('screenshotEditor.actions.scrollDown')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.scrollDown')}
          disabled={isSaving || !imageLoaded}
          onPointerDown={(event) => {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            onScrollStart('down');
          }}
          onPointerUp={onScrollStop}
          onPointerCancel={onScrollStop}
          className={toolbarButtonClass}
        >
          <ChevronDown size={18} />
        </button>
      </TooltipButton>
      <TooltipButton label={t('screenshotEditor.actions.cancel')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.cancel')}
          onClick={onCancel}
          className="flex h-8 w-8 items-center justify-center rounded-md border border-red-600 bg-red-600 text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <X size={18} />
        </button>
      </TooltipButton>
      <TooltipButton label={t('screenshotEditor.actions.confirm')} positionClass="bottom-full mb-2" useNativeTooltip={useNativeTooltip}>
        <button
          type="button"
          aria-label={t('screenshotEditor.actions.confirm')}
          disabled={isSaving || !imageLoaded}
          onClick={onConfirm}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-green-600 text-white transition-colors hover:bg-green-700 disabled:opacity-50"
        >
          <Check size={18} />
        </button>
      </TooltipButton>
    </div>
  );
}

function TooltipButton({
  label,
  positionClass,
  useNativeTooltip = false,
  children,
}: {
  label: string;
  positionClass: string;
  useNativeTooltip?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="group relative shrink-0" title={useNativeTooltip ? label : undefined}>
      {children}
      {!useNativeTooltip && <ToolbarTooltip label={label} positionClass={positionClass} />}
    </div>
  );
}

function ToolbarTooltip({ label, positionClass }: { label: string; positionClass: string }) {
  return <span className={`${toolbarTooltipClass} ${positionClass}`}>{label}</span>;
}

function ToolOptionPanel({
  label,
  positionClass,
  children,
}: {
  label: string;
  positionClass: string;
  children: React.ReactNode;
}) {
  return (
    <div className={`${toolOptionPanelClass} ${positionClass}`} onPointerDown={(event) => event.stopPropagation()}>
      <div className="mb-1 text-xs font-semibold text-on-surface-variant">{label}</div>
      {children}
    </div>
  );
}

function CheckerboardIcon({ size = 18, active = false }: { size?: number; active?: boolean }) {
  const darkClass = active ? 'bg-white' : 'bg-black';
  const lightClass = active ? 'bg-black' : 'bg-white';
  return (
    <span
      aria-hidden="true"
      className="grid grid-cols-2 overflow-hidden rounded-sm border border-current"
      style={{
        width: size,
        height: size,
      }}
    >
      <span className={darkClass} />
      <span className={lightClass} />
      <span className={lightClass} />
      <span className={darkClass} />
    </span>
  );
}

function LongScreenshotIcon() {
  return (
    <span className="relative flex h-[18px] w-[14px] items-center justify-center rounded-[2px] border border-current">
      <ArrowUpDown size={12} strokeWidth={2.5} />
    </span>
  );
}
