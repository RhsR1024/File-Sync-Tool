import { describe, expect, it } from 'vitest';

import {
  computeContainedRect,
  normalizedToCss,
  normalizedToDevice,
  pointToNormalized,
} from './coordinates';

describe('screen share coordinate mapping', () => {
  it('excludes letterbox bars for a wide screen inside a square viewport', () => {
    const rect = computeContainedRect(1000, 1000, 1920, 1080, 2);

    expect(rect.left).toBe(0);
    expect(rect.top).toBe(218.75);
    expect(rect.width).toBe(1000);
    expect(rect.height).toBe(562.5);
    expect(rect.deviceWidth).toBe(2000);
    expect(rect.deviceHeight).toBe(1125);

    expect(pointToNormalized(500, 500, { left: 0, top: 0 }, rect)).toEqual({
      x: 0.5,
      y: 0.5,
    });
    expect(pointToNormalized(500, 100, { left: 0, top: 0 }, rect)).toBeNull();
    expect(pointToNormalized(500, 900, { left: 0, top: 0 }, rect)).toBeNull();
  });

  it('maps normalized points to CSS and device pixels consistently', () => {
    const rect = computeContainedRect(800, 600, 1600, 900, 1.5);
    const point = { x: 0.25, y: 0.75 };

    expect(normalizedToCss(point, rect)).toEqual({ x: 200, y: 412.5 });
    expect(normalizedToDevice(point, rect)).toEqual({ x: 300, y: 618.75 });
  });

  it('returns a stable empty rectangle before the MJPEG dimensions are known', () => {
    const rect = computeContainedRect(800, 600, 0, 0, 2);

    expect(rect.width).toBe(0);
    expect(rect.height).toBe(0);
    expect(pointToNormalized(10, 10, { left: 0, top: 0 }, rect)).toBeNull();
  });
});
