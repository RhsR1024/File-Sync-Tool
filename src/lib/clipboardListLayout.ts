import type { ClipboardDisplaySettings, ClipboardItem } from './clipboardTypes';

interface ClipboardItemHeightOptions {
  compact?: boolean;
}

function previewLineCount(settings: ClipboardDisplaySettings): number {
  return Math.max(1, Math.min(settings.preview_lines ?? 3, 6));
}

function densityHeightAdjust(settings: ClipboardDisplaySettings): number {
  switch (settings.density) {
    case 'compact':
      return -8;
    case 'spacious':
      return 16;
    case 'standard':
    default:
      return 0;
  }
}

export function resolveClipboardItemHeight(
  item: ClipboardItem,
  settings: ClipboardDisplaySettings,
  options: ClipboardItemHeightOptions = {},
): number {
  const densityAdjust = densityHeightAdjust(settings);
  const compact = options.compact ?? false;

  if (item.kind === 'image') {
    const fixedHeightAdjust = settings.image_auto_height ? 0 : 18;
    return (compact ? 148 : 168) + densityAdjust + fixedHeightAdjust;
  }
  if (item.kind === 'file') {
    return (compact ? 80 : 96) + densityAdjust;
  }

  const lineAdjust = Math.max(0, previewLineCount(settings) - 2) * 18;
  return (compact ? 72 : 88) + densityAdjust + lineAdjust;
}

export function resolveClipboardPinnedSectionHeight(
  items: ClipboardItem[],
  settings: ClipboardDisplaySettings,
  options: ClipboardItemHeightOptions = {},
): number {
  const visibleRows = Math.min(items.length, options.compact ? 2 : 3);
  return items
    .slice(0, visibleRows)
    .reduce(
      (total, item) =>
        total + resolveClipboardItemHeight(item, settings, options),
      0,
    );
}
