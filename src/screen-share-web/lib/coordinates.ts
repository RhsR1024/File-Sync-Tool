import type { NormalizedPoint } from '../types';

export interface ContainedRect {
  left: number;
  top: number;
  width: number;
  height: number;
  dpr: number;
  deviceLeft: number;
  deviceTop: number;
  deviceWidth: number;
  deviceHeight: number;
}

/**
 * Returns the CSS and device-pixel rectangle occupied by an object-fit: contain
 * image. The surrounding letterbox is deliberately excluded from hit testing.
 */
export function computeContainedRect(
  containerWidth: number,
  containerHeight: number,
  naturalWidth: number,
  naturalHeight: number,
  devicePixelRatio = 1,
): ContainedRect {
  const cw = Math.max(0, Number.isFinite(containerWidth) ? containerWidth : 0);
  const ch = Math.max(0, Number.isFinite(containerHeight) ? containerHeight : 0);
  const iw = Math.max(0, Number.isFinite(naturalWidth) ? naturalWidth : 0);
  const ih = Math.max(0, Number.isFinite(naturalHeight) ? naturalHeight : 0);
  const dpr = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;

  if (!cw || !ch || !iw || !ih) {
    return {
      left: 0,
      top: 0,
      width: 0,
      height: 0,
      dpr,
      deviceLeft: 0,
      deviceTop: 0,
      deviceWidth: 0,
      deviceHeight: 0,
    };
  }

  const scale = Math.min(cw / iw, ch / ih);
  const width = cleanNumber(iw * scale);
  const height = cleanNumber(ih * scale);
  const left = cleanNumber((cw - width) / 2);
  const top = cleanNumber((ch - height) / 2);

  return {
    left,
    top,
    width,
    height,
    dpr,
    deviceLeft: left * dpr,
    deviceTop: top * dpr,
    deviceWidth: width * dpr,
    deviceHeight: height * dpr,
  };
}

function cleanNumber(value: number): number {
  if (Math.abs(value) < 1e-9) return 0;
  return Math.round(value * 1e9) / 1e9;
}

export function pointToNormalized(
  clientX: number,
  clientY: number,
  container: DOMRect | { left: number; top: number },
  imageRect: ContainedRect,
): NormalizedPoint | null {
  if (imageRect.width <= 0 || imageRect.height <= 0) {
    return null;
  }

  const x = clientX - container.left - imageRect.left;
  const y = clientY - container.top - imageRect.top;
  const nx = x / imageRect.width;
  const ny = y / imageRect.height;

  if (!Number.isFinite(nx) || !Number.isFinite(ny) || nx < 0 || nx > 1 || ny < 0 || ny > 1) {
    return null;
  }

  return { x: nx, y: ny };
}

export function normalizedToCss(point: NormalizedPoint, imageRect: ContainedRect): { x: number; y: number } {
  return {
    x: imageRect.left + point.x * imageRect.width,
    y: imageRect.top + point.y * imageRect.height,
  };
}

export function normalizedToDevice(point: NormalizedPoint, imageRect: ContainedRect): { x: number; y: number } {
  return {
    x: imageRect.deviceLeft + point.x * imageRect.deviceWidth,
    y: imageRect.deviceTop + point.y * imageRect.deviceHeight,
  };
}

export function clampNormalized(point: NormalizedPoint): NormalizedPoint {
  return {
    x: Math.min(1, Math.max(0, Number.isFinite(point.x) ? point.x : 0)),
    y: Math.min(1, Math.max(0, Number.isFinite(point.y) ? point.y : 0)),
  };
}
