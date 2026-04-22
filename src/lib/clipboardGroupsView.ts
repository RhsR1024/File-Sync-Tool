import type { ClipboardGroup, ClipboardItem } from './clipboardTypes';

export function partitionClipboardItemsForDisplay<T extends ClipboardItem>(items: T[]): {
  pinnedItems: T[];
  regularItems: T[];
} {
  const pinnedItems: T[] = [];
  const regularItems: T[] = [];

  for (const item of items) {
    if (item.is_pinned) pinnedItems.push(item);
    else regularItems.push(item);
  }

  return { pinnedItems, regularItems };
}

export function resolveActiveClipboardGroupId(
  groups: ClipboardGroup[],
  selectedGroupId: number | null,
): number | null {
  if (selectedGroupId === null) return null;
  return groups.some((group) => group.id === selectedGroupId) ? selectedGroupId : null;
}
