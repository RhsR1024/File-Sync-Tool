import type { ClipboardItem, FilePathStatus } from '../lib/clipboardTypes';

export type ClipboardContextActionId =
  | 'paste'
  | 'pastePlain'
  | 'pasteAsFiles'
  | 'pasteAsPath'
  | 'copy'
  | 'showFileDetails'
  | 'openInExplorer'
  | 'saveImageAs'
  | 'toggleFavorite'
  | 'delete';

export interface ClipboardContextMenuItem {
  id: ClipboardContextActionId;
  labelKey: string;
  disabled?: boolean;
  destructive?: boolean;
}

interface BuildClipboardMenuItemsOptions {
  item: ClipboardItem;
  fileStatuses?: FilePathStatus[] | null;
}

function hasAtLeastOneExistingPath(statuses?: FilePathStatus[] | null): boolean {
  return (statuses ?? []).some((status) => status.exists);
}

function pushToggleFavorite(items: ClipboardContextMenuItem[], item: ClipboardItem): void {
  items.push({
    id: 'toggleFavorite',
    labelKey: item.is_favorite
      ? 'clipboard.actions.unfavorite'
      : 'clipboard.actions.favorite',
  });
}

function pushDelete(items: ClipboardContextMenuItem[]): void {
  items.push({
    id: 'delete',
    labelKey: 'clipboard.actions.delete',
    destructive: true,
  });
}

export function buildClipboardMenuItems({
  item,
  fileStatuses,
}: BuildClipboardMenuItemsOptions): ClipboardContextMenuItem[] {
  const items: ClipboardContextMenuItem[] = [];

  if (item.kind === 'file') {
    const hasExistingPaths = hasAtLeastOneExistingPath(fileStatuses);
    items.push(
      {
        id: 'pasteAsFiles',
        labelKey: 'clipboard.actions.pasteAsFiles',
        disabled: !hasExistingPaths,
      },
      {
        id: 'pasteAsPath',
        labelKey: 'clipboard.actions.pasteAsPath',
        disabled: false,
      },
      {
        id: 'copy',
        labelKey: 'clipboard.actions.copy',
      },
      {
        id: 'showFileDetails',
        labelKey: 'clipboard.actions.showFileDetails',
        disabled: false,
      },
      {
        id: 'openInExplorer',
        labelKey: 'clipboard.actions.openInExplorer',
        disabled: !hasExistingPaths,
      },
    );
    pushToggleFavorite(items, item);
    pushDelete(items);
    return items;
  }

  if (item.kind === 'image') {
    items.push(
      {
        id: 'paste',
        labelKey: 'clipboard.actions.paste',
      },
      {
        id: 'copy',
        labelKey: 'clipboard.actions.copy',
      },
      {
        id: 'saveImageAs',
        labelKey: 'clipboard.actions.saveImageAs',
        disabled: !item.image_path,
      },
    );
    pushToggleFavorite(items, item);
    pushDelete(items);
    return items;
  }

  items.push(
    {
      id: 'paste',
      labelKey: 'clipboard.actions.paste',
    },
    {
      id: 'pastePlain',
      labelKey: 'clipboard.actions.pastePlain',
    },
    {
      id: 'copy',
      labelKey: 'clipboard.actions.copy',
    },
  );
  pushToggleFavorite(items, item);
  pushDelete(items);
  return items;
}

function getPathSeparator(directory: string): string {
  return directory.includes('\\') ? '\\' : '/';
}

function sanitizeFileName(name: string): string {
  const sanitized = name
    .replace(/[<>:"/\\|?*\u0000-\u001F]/g, '-')
    .replace(/\s+/g, ' ')
    .trim();
  return sanitized || 'clipboard-image.png';
}

function getLastPathSegment(path: string): string {
  const normalized = path.replace(/[\\/]+$/, '');
  const segments = normalized.split(/[/\\]/).filter(Boolean);
  return segments.at(-1) ?? '';
}

export function buildImageSaveTargetPath(directory: string, item: ClipboardItem): string {
  const trimmedDirectory = directory.replace(/[\\/]+$/, '');
  const sourceName = item.image_path ? getLastPathSegment(item.image_path) : '';
  const baseName = sourceName || `clipboard-image-${item.id}.png`;
  return `${trimmedDirectory}${getPathSeparator(trimmedDirectory)}${sanitizeFileName(baseName)}`;
}

export function decodeMergeSeparatorInput(input: string): string {
  if (!input) return '\n';

  return input
    .replace(/\\r/g, '\r')
    .replace(/\\n/g, '\n')
    .replace(/\\t/g, '\t');
}

export function getPreferredExplorerPath(
  item: ClipboardItem,
  statuses: FilePathStatus[] | null,
): string | null {
  if (item.kind === 'image') return item.image_path;
  const existing = (statuses ?? []).find((status) => status.exists);
  if (existing) return existing.path;
  return item.file_paths?.[0] ?? null;
}
