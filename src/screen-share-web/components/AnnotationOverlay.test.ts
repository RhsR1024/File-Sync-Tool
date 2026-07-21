import { createApp, h, nextTick } from 'vue';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AnnotationShape, AnnotationUpdatePayload } from '../types';
import AnnotationOverlay from './AnnotationOverlay.vue';

const geometry = {
  left: 0,
  top: 0,
  width: 100,
  height: 100,
  dpr: 1,
  deviceLeft: 0,
  deviceTop: 0,
  deviceWidth: 100,
  deviceHeight: 100,
};

const arrow: AnnotationShape = {
  id: 'shape-a',
  ownerClientId: 'viewer-a',
  kind: 'arrow',
  points: [{ x: 0.1, y: 0.2 }, { x: 0.7, y: 0.8 }],
  color: '#ef4444',
  width: 4,
  expiresAtMs: null,
};

let cleanup: (() => void) | null = null;

function pointerEvent(type: string, x: number, y: number): MouseEvent {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: x,
    clientY: y,
  });
  Object.defineProperty(event, 'pointerId', { value: 9 });
  return event;
}

async function mountOverlay(
  shape: AnnotationShape,
  events: Array<string | AnnotationUpdatePayload>,
) {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const app = createApp({
    render: () => h('div', [
      h(AnnotationOverlay, {
        shapes: [shape],
        geometry,
        tool: 'view',
        color: '#f59e0b',
        width: 4,
        enabled: true,
        editMode: true,
        selectedId: shape.id,
        clientId: 'viewer-a',
        onSelect: (shapeId: string | null) => events.push(`select:${shapeId ?? 'none'}`),
        onUpdate: (payload: AnnotationUpdatePayload) => events.push(payload),
      }),
    ]),
  });
  app.mount(host);
  await nextTick();

  const parent = host.firstElementChild as HTMLElement;
  const layer = parent.firstElementChild as HTMLElement & {
    setPointerCapture: (pointerId: number) => void;
    releasePointerCapture: (pointerId: number) => void;
  };
  parent.getBoundingClientRect = () => ({ left: 0, top: 0 } as DOMRect);
  layer.setPointerCapture = vi.fn();
  layer.releasePointerCapture = vi.fn();

  cleanup = () => {
    app.unmount();
    host.remove();
  };
  return layer;
}

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

describe('AnnotationOverlay editing', () => {
  it('moves an owned annotation and emits one server update on pointer up', async () => {
    const events: Array<string | AnnotationUpdatePayload> = [];
    const layer = await mountOverlay(arrow, events);
    const hitTarget = layer.querySelector('.annotation-hit-target') as SVGElement;

    expect(hitTarget).not.toBeNull();
    expect(layer.querySelector('.laser-dot')).toBeNull();
    hitTarget.dispatchEvent(pointerEvent('pointerdown', 10, 20));
    layer.dispatchEvent(pointerEvent('pointermove', 20, 30));
    layer.dispatchEvent(pointerEvent('pointerup', 20, 30));

    expect(events[0]).toBe('select:shape-a');
    const update = events[1] as AnnotationUpdatePayload;
    expect(update).toMatchObject({
      shape_id: 'shape-a',
      color: '#ef4444',
      width: 4,
    });
    expect(update.points[0].x).toBeCloseTo(0.2);
    expect(update.points[0].y).toBeCloseTo(0.3);
    expect(update.points[1].x).toBeCloseTo(0.8);
    expect(update.points[1].y).toBeCloseTo(0.9);
  });

  it('does not allow editing another viewers annotation', async () => {
    const events: Array<string | AnnotationUpdatePayload> = [];
    const layer = await mountOverlay({ ...arrow, ownerClientId: 'viewer-b' }, events);
    const shape = layer.querySelector('.annotation-shape') as SVGElement;

    shape.dispatchEvent(pointerEvent('pointerdown', 10, 20));
    layer.dispatchEvent(pointerEvent('pointermove', 20, 30));
    layer.dispatchEvent(pointerEvent('pointerup', 20, 30));

    expect(events).toEqual(['select:none']);
  });
});
