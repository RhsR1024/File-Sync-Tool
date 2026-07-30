import { describe, expect, it } from 'vitest';

import { annotationBarView, carryDismissal, persistentShapes } from './annotation-bar';
import { emptyDocument } from './annotation-state';
import type { AnnotationDocument, AnnotationKind, AnnotationShape } from '../types';

function shape(id: string, kind: AnnotationKind = 'arrow'): AnnotationShape {
  return {
    id,
    ownerClientId: 'viewer-1',
    kind,
    points: [{ x: 0.1, y: 0.1 }, { x: 0.4, y: 0.4 }],
    color: '#f59e0b',
    width: 4,
    expiresAtMs: kind === 'laser' ? Date.now() + 1_000 : null,
  };
}

function documentWith(...shapes: AnnotationShape[]): AnnotationDocument {
  return { ...emptyDocument(7, 1), revision: shapes.length, shapes };
}

describe('persistentShapes', () => {
  it('ignores laser points because they expire without host action', () => {
    const document = documentWith(shape('a'), shape('laser-1', 'laser'), shape('b', 'rect'));
    expect(persistentShapes(document).map((entry) => entry.id)).toEqual(['a', 'b']);
  });
});

describe('annotationBarView', () => {
  it('stays hidden while there is nothing to clear', () => {
    expect(annotationBarView(emptyDocument(), null)).toEqual({
      count: 0,
      latestShapeId: null,
      visible: false,
    });
  });

  it('stays hidden for a laser point alone', () => {
    const view = annotationBarView(documentWith(shape('laser-1', 'laser')), null);
    expect(view).toEqual({ count: 0, latestShapeId: null, visible: false });
  });

  it('shows once a viewer leaves a persistent annotation, targeting the newest one', () => {
    const view = annotationBarView(documentWith(shape('a'), shape('b')), null);
    expect(view).toEqual({ count: 2, latestShapeId: 'b', visible: true });
  });

  it('hides again after the host clears everything', () => {
    expect(annotationBarView(documentWith(), null).visible).toBe(false);
  });

  it('stays hidden at the dismissed count and returns for the next annotation', () => {
    const dismissed = documentWith(shape('a'), shape('b'));
    expect(annotationBarView(dismissed, 2).visible).toBe(false);

    const oneMore = documentWith(shape('a'), shape('b'), shape('c'));
    expect(annotationBarView(oneMore, 2)).toEqual({
      count: 3,
      latestShapeId: 'c',
      visible: true,
    });
  });
});

describe('carryDismissal', () => {
  it('expires the dismissal once every annotation is gone', () => {
    expect(carryDismissal(0, 4)).toBeNull();
  });

  it('clamps down when annotations are removed so new ones still surface', () => {
    // Dismissed at five, then three were removed: the next new annotation must
    // reach a threshold of two, not five.
    expect(carryDismissal(2, 5)).toBe(2);
    expect(annotationBarView(documentWith(shape('a'), shape('b'), shape('c')), 2).visible).toBe(true);
  });

  it('leaves an untouched dismissal alone', () => {
    expect(carryDismissal(5, 5)).toBe(5);
    expect(carryDismissal(6, 5)).toBe(5);
  });

  it('reports no dismissal when none was recorded', () => {
    expect(carryDismissal(3, null)).toBeNull();
  });
});
