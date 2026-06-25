export type Point = { x: number; y: number };

export type Rect = { x: number; y: number; width: number; height: number };

export type Tool = 'pen' | 'line' | 'arrow' | 'ellipse' | 'rect' | 'mosaic' | 'text' | 'emoji' | 'number';

export type ResizeHandle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

export type Annotation =
  | { kind: 'pen'; points: Point[]; color: string; width: number }
  | { kind: 'line'; from: Point; to: Point; color: string; width: number }
  | { kind: 'arrow'; from: Point; to: Point; color: string; width: number }
  | { kind: 'ellipse'; rect: Rect; color: string; width: number }
  | { kind: 'rect'; rect: Rect; color: string; width: number }
  | { kind: 'mosaic'; points: Point[]; width: number; blockSize: number }
  | { kind: 'text'; position: Point; text: string; color: string; fontSize: number }
  | { kind: 'emoji'; rect: Rect; emoji: string }
  | { kind: 'number-tag'; rect: Rect; number: number; color: string };

export type TextDraft = {
  position: Point;
  cssPosition: Point;
  value: string;
};

export type AnnotationDragState = {
  annotationIndex: number;
  initialAnnotations: Annotation[];
  initialRect: Rect;
  startPoint: Point;
  mode: 'move' | 'resize';
  handle?: ResizeHandle;
};
