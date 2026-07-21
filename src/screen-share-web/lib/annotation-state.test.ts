import { describe, expect, it } from 'vitest';

import {
  applyAnnotationApplied,
  applySnapshot,
  applyViewState,
  emptyDocument,
  normalizeShape,
  resetForSource,
  visibleShapes,
} from './annotation-state';

const snakeArrow = {
  id: 'shape-1',
  owner_client_id: 'client-a',
  kind: 'arrow',
  points: [{ x: 0.1, y: 0.2 }, { x: 0.7, y: 0.8 }],
  color: '#38bdf8',
  width: 4,
  expires_at_ms: null,
};

describe('screen share annotation state', () => {
  it('normalizes the Rust snake_case shape contract', () => {
    expect(normalizeShape(snakeArrow)).toMatchObject({
      id: 'shape-1',
      ownerClientId: 'client-a',
      kind: 'arrow',
      points: snakeArrow.points,
    });
  });

  it('replaces state from a full server snapshot', () => {
    const current = emptyDocument(42, 7);
    const next = applySnapshot(current, {
      session_id: 42,
      source_epoch: 7,
      revision: 3,
      mode: 'frozen',
      frozen_frame_id: 19,
      shapes: [snakeArrow],
    });

    expect(next).toMatchObject({ sessionId: 42, sourceEpoch: 7, revision: 3, mode: 'frozen', frozenFrameId: 19 });
    expect(next.shapes).toHaveLength(1);
  });

  it('accepts a full document carried by annotation.applied and detects revision gaps for deltas', () => {
    const current = {
      ...emptyDocument(42, 7),
      revision: 1,
    };
    const full = applyAnnotationApplied(current, { document: {
      session_id: 42,
      source_epoch: 7,
      revision: 2,
      mode: 'live',
      frozen_frame_id: null,
      shapes: [snakeArrow],
    } }, 2);
    expect(full.needsSnapshot).toBe(false);
    expect(full.document.revision).toBe(2);

    const gap = applyAnnotationApplied(current, { operation: 'add', shape: snakeArrow }, 4);
    expect(gap.needsSnapshot).toBe(true);
  });

  it('applies a single annotation update delta without changing ownership', () => {
    const current = applySnapshot(emptyDocument(42, 7), {
      session_id: 42,
      source_epoch: 7,
      revision: 1,
      mode: 'live',
      frozen_frame_id: null,
      shapes: [snakeArrow],
    });
    const result = applyAnnotationApplied(current, {
      operation: 'update',
      shape: {
        ...snakeArrow,
        points: [{ x: 0.2, y: 0.25 }, { x: 0.8, y: 0.75 }],
        color: '#22c55e',
        width: 7,
      },
    }, 2);

    expect(result.needsSnapshot).toBe(false);
    expect(result.document.shapes[0]).toMatchObject({
      id: 'shape-1',
      ownerClientId: 'client-a',
      color: '#22c55e',
      width: 7,
    });
  });

  it('removes non-laser annotations when returning to live view', () => {
    const frozen = applySnapshot(emptyDocument(42, 7), {
      session_id: 42,
      source_epoch: 7,
      revision: 2,
      mode: 'frozen',
      frozen_frame_id: 19,
      shapes: [snakeArrow, { ...snakeArrow, id: 'laser-1', kind: 'laser', points: [{ x: 0.2, y: 0.2 }], expires_at_ms: Date.now() + 1000 }],
    });
    const live = applyViewState(frozen, 'live', null);
    expect(live.mode).toBe('live');
    expect(live.frozenFrameId).toBeNull();
    expect(live.shapes.map((shape) => shape.id)).toEqual(['laser-1']);
  });

  it('clears all state on a capture source epoch change', () => {
    const current = applySnapshot(emptyDocument(42, 7), {
      session_id: 42,
      source_epoch: 7,
      revision: 2,
      mode: 'frozen',
      frozen_frame_id: 19,
      shapes: [snakeArrow],
    });
    const reset = resetForSource(current, 8);
    expect(reset).toMatchObject({ sourceEpoch: 8, revision: 0, mode: 'live', frozenFrameId: null });
    expect(reset.shapes).toEqual([]);
  });

  it('hides expired laser points without mutating the document', () => {
    const current = applySnapshot(emptyDocument(42, 7), {
      session_id: 42,
      source_epoch: 7,
      revision: 1,
      mode: 'live',
      frozen_frame_id: null,
      shapes: [{ ...snakeArrow, id: 'laser-1', kind: 'laser', points: [{ x: 0.2, y: 0.2 }], expires_at_ms: 100 }],
    });
    expect(visibleShapes(current, 101)).toEqual([]);
    expect(current.shapes).toHaveLength(1);
  });
});
