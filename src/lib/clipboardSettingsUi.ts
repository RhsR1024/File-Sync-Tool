import type { ClipboardToolbarSettings } from './clipboardTypes.ts';

export const CLIPBOARD_SETTINGS_TABS = [
  { id: 'general', labelKey: 'clipboard.settings.tabs.general' },
  { id: 'display', labelKey: 'clipboard.settings.tabs.display' },
  { id: 'shortcuts', labelKey: 'clipboard.settings.tabs.shortcuts' },
  { id: 'data', labelKey: 'clipboard.settings.tabs.data' },
  { id: 'preview', labelKey: 'clipboard.settings.tabs.preview' },
  { id: 'appFilter', labelKey: 'clipboard.settings.tabs.appFilter' },
  { id: 'audio', labelKey: 'clipboard.settings.tabs.audio' },
  { id: 'about', labelKey: 'clipboard.settings.tabs.about' },
] as const;

export type ClipboardSettingsTabId =
  typeof CLIPBOARD_SETTINGS_TABS[number]['id'];

export const CLIPBOARD_TOOLBAR_SECTION_IDS = ['search', 'filter'] as const;
export const CLIPBOARD_TOOLBAR_ACTION_IDS = [
  'batch',
  'settings',
  'lock',
] as const;
export const CLIPBOARD_TOOLBAR_ITEM_IDS = [
  ...CLIPBOARD_TOOLBAR_SECTION_IDS,
  ...CLIPBOARD_TOOLBAR_ACTION_IDS,
] as const;

export type ClipboardToolbarSectionId =
  typeof CLIPBOARD_TOOLBAR_SECTION_IDS[number];
export type ClipboardToolbarActionId =
  typeof CLIPBOARD_TOOLBAR_ACTION_IDS[number];
export type ClipboardToolbarItemId = typeof CLIPBOARD_TOOLBAR_ITEM_IDS[number];

const CLIPBOARD_TOOLBAR_ITEM_SET = new Set<string>(CLIPBOARD_TOOLBAR_ITEM_IDS);
const DEFAULT_TOOLBAR_ITEMS: ClipboardToolbarItemId[] = [
  'search',
  'filter',
  'batch',
  'settings',
  'lock',
];

export interface ClipboardToolbarLayout {
  showSearch: boolean;
  showFilter: boolean;
  actionItems: ClipboardToolbarActionId[];
}

export function normalizeClipboardToolbarItems(
  items: readonly string[],
): ClipboardToolbarItemId[] {
  const next: ClipboardToolbarItemId[] = [];

  for (const item of items) {
    if (!CLIPBOARD_TOOLBAR_ITEM_SET.has(item) || next.includes(item as ClipboardToolbarItemId)) {
      continue;
    }
    next.push(item as ClipboardToolbarItemId);
  }

  return next.length > 0 ? next : [...DEFAULT_TOOLBAR_ITEMS];
}

export function moveClipboardToolbarItem(
  items: readonly string[],
  item: ClipboardToolbarItemId,
  direction: -1 | 1,
): ClipboardToolbarItemId[] {
  const normalized = normalizeClipboardToolbarItems(items);
  const index = normalized.indexOf(item);
  if (index < 0) return normalized;

  const nextIndex = index + direction;
  if (nextIndex < 0 || nextIndex >= normalized.length) {
    return normalized;
  }

  const next = [...normalized];
  const [moved] = next.splice(index, 1);
  next.splice(nextIndex, 0, moved);
  return next;
}

export function buildClipboardToolbarLayout(
  settings: ClipboardToolbarSettings,
  supportedActionItems: readonly ClipboardToolbarActionId[],
): ClipboardToolbarLayout {
  if (!settings.visible) {
    return {
      showSearch: false,
      showFilter: false,
      actionItems: [],
    };
  }

  const normalized = normalizeClipboardToolbarItems(settings.items);
  const supportedActions = new Set<ClipboardToolbarActionId>(supportedActionItems);

  return {
    showSearch: normalized.includes('search'),
    showFilter: normalized.includes('filter'),
    actionItems: normalized.filter(
      (item): item is ClipboardToolbarActionId =>
        (CLIPBOARD_TOOLBAR_ACTION_IDS as readonly string[]).includes(item)
        && supportedActions.has(item as ClipboardToolbarActionId),
    ),
  };
}
