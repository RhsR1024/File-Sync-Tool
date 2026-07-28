import { createApp, h, nextTick } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import RemoteControlOverlay from './RemoteControlOverlay.vue';

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

let cleanup: (() => void) | null = null;
let animationFrameCallback: FrameRequestCallback | null = null;

function pointerEvent(type: string, x: number, y: number, button = 0, timeStamp = 100): MouseEvent {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: x,
    clientY: y,
    button,
  });
  Object.defineProperty(event, 'pointerId', { value: 7 });
  Object.defineProperty(event, 'timeStamp', { value: timeStamp });
  return event;
}

async function mountOverlay(events: string[]) {
  const host = document.createElement('div');
  document.body.appendChild(host);

  const app = createApp({
    render: () => h('div', [
      h(RemoteControlOverlay, {
        geometry,
        enabled: true,
        onMove: (point: { x: number; y: number }, timeStamp: number) => events.push(`move:${point.x},${point.y}:${timeStamp}`),
        onButton: (payload: { button: string; pressed: boolean }, timeStamp?: number) => events.push(`button:${payload.button}:${payload.pressed}:${timeStamp ?? 'none'}`),
        onWheel: (delta: number) => events.push(`wheel:${delta}`),
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
  layer.getBoundingClientRect = () => ({
    left: 0,
    top: 0,
    right: 100,
    bottom: 100,
    width: 100,
    height: 100,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  layer.setPointerCapture = vi.fn();
  layer.releasePointerCapture = vi.fn();

  cleanup = () => {
    app.unmount();
    host.remove();
  };
  return layer;
}

describe('RemoteControlOverlay input ordering', () => {
  beforeEach(() => {
    animationFrameCallback = null;
    vi.stubGlobal('requestAnimationFrame', vi.fn((callback: FrameRequestCallback) => {
      animationFrameCallback = callback;
      return 41;
    }));
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
  });

  afterEach(() => {
    cleanup?.();
    cleanup = null;
    animationFrameCallback = null;
    vi.unstubAllGlobals();
  });

  it('sends the current position before mouse down and mouse up', async () => {
    const events: string[] = [];
    const layer = await mountOverlay(events);

    layer.dispatchEvent(pointerEvent('pointerdown', 25, 30, 0, 101));
    expect(events).toEqual(['move:0.25,0.3:101', 'button:left:true:101']);

    events.length = 0;
    layer.dispatchEvent(pointerEvent('pointermove', 50, 60, 0, 102));
    layer.dispatchEvent(pointerEvent('pointerup', 75, 80, 0, 103));

    expect(cancelAnimationFrame).toHaveBeenCalledWith(41);
    expect(events).toEqual(['move:0.75,0.8:103', 'button:left:false:103']);
  });

  it('sends the pointer position before wheel input', async () => {
    const events: string[] = [];
    const layer = await mountOverlay(events);

    layer.dispatchEvent(new WheelEvent('wheel', {
      bubbles: true,
      cancelable: true,
      clientX: 40,
      clientY: 55,
      deltaY: 120,
    }));

    expect(events[0]).toMatch(/^move:0\.4,0\.55:\d+(?:\.\d+)?$/);
    expect(events[1]).toBe('wheel:-120');
  });

  it('keeps only the newest pointer move and its timestamp until the next animation frame', async () => {
    const events: string[] = [];
    const layer = await mountOverlay(events);

    layer.dispatchEvent(pointerEvent('pointermove', 20, 30, 0, 201));
    layer.dispatchEvent(pointerEvent('pointermove', 70, 80, 0, 202));
    expect(events).toEqual([]);

    animationFrameCallback?.(16);
    expect(events).toEqual(['move:0.7,0.8:202']);
  });
});
