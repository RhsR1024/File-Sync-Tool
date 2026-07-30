import type { AnnotationDocument, AnnotationShape } from '../types';

/**
 * Rules for the host's annotation action bar, kept beside `annotation-state` so
 * both the viewer overlay and the host desktop pages read the same document
 * shape. The bar is host-only chrome, but every input it needs is already in the
 * shared `AnnotationDocument`.
 */
export interface AnnotationBarView {
  /** Annotations worth acting on. Laser points expire on their own. */
  count: number;
  /** Target of "clear last"; null when there is nothing to undo. */
  latestShapeId: string | null;
  visible: boolean;
}

/** Annotations that persist until somebody removes them. */
export function persistentShapes(document: AnnotationDocument): AnnotationShape[] {
  return document.shapes.filter((shape) => shape.kind !== 'laser');
}

/**
 * Carry a dismissal across annotation changes.
 *
 * A dismissal means "hide until something new arrives", so it expires once
 * every annotation is gone. It also clamps down when annotations are removed:
 * without that, dismissing at five and then removing two would leave the bar
 * hidden for the next three new annotations.
 */
export function carryDismissal(count: number, dismissedAtCount: number | null): number | null {
  if (count === 0 || dismissedAtCount === null) return null;
  return Math.min(dismissedAtCount, count);
}

export function annotationBarView(
  document: AnnotationDocument,
  dismissedAtCount: number | null,
): AnnotationBarView {
  const shapes = persistentShapes(document);
  const count = shapes.length;
  const dismissal = carryDismissal(count, dismissedAtCount);
  return {
    count,
    latestShapeId: shapes[count - 1]?.id ?? null,
    visible: count > 0 && (dismissal === null || count > dismissal),
  };
}
