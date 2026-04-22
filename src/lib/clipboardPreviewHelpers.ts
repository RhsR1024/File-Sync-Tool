export type HoverPreviewKind = 'image' | 'text';

export interface HoverPreviewCandidate {
  id: number;
  kind: string;
}

export interface HoverPreviewTarget {
  id: number;
  kind: HoverPreviewKind;
}

export interface ClipboardImagePreviewPayload {
  id: number;
  image_path: string;
  zoom_step: number;
  source_app: string | null;
}

export interface ClipboardTextPreviewPayload {
  id: number;
  kind: string;
  content: string;
  source_app: string | null;
}

export const IMAGE_PREVIEW_UPDATE_EVENT = 'clipboard-image-preview-update';
export const TEXT_PREVIEW_UPDATE_EVENT = 'clipboard-text-preview-update';
export const DEFAULT_IMAGE_PREVIEW_SCALE = 1;
export const MIN_IMAGE_PREVIEW_SCALE = 0.25;
export const MAX_IMAGE_PREVIEW_SCALE = 6;

export function resolveHoverPreviewTarget(
  item: HoverPreviewCandidate | null | undefined,
): HoverPreviewTarget | null {
  if (!item) return null;

  if (item.kind === 'image') {
    return {
      id: item.id,
      kind: 'image',
    };
  }

  if (item.kind === 'text' || item.kind === 'html' || item.kind === 'rtf') {
    return {
      id: item.id,
      kind: 'text',
    };
  }

  return null;
}

export function clampImagePreviewScale(
  value: number,
  min = MIN_IMAGE_PREVIEW_SCALE,
  max = MAX_IMAGE_PREVIEW_SCALE,
): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, value));
}

export function stepImagePreviewScale(
  current: number,
  direction: 1 | -1,
  zoomStep: number,
): number {
  const delta = Math.max(1, zoomStep) / 100;
  return clampImagePreviewScale(current + direction * delta);
}
