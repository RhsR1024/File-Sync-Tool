export type ClipboardContentKind = 'text' | 'html' | 'rtf' | 'image' | 'file';
export type ClipboardFilter = 'all' | 'text' | 'image' | 'file' | 'favorite' | 'pinned';
export type ClipboardDedupStrategy = 'move_to_top' | 'ignore' | 'always_new';
export type ClipboardCardDensity = 'compact' | 'standard' | 'spacious';
export type ClipboardTimeFormat = 'relative' | 'absolute';
export type ClipboardSourceAppDisplay = 'none' | 'name' | 'icon' | 'both';
export type ClipboardPreviewPosition = 'auto' | 'left' | 'right';
export type ClipboardAppFilterMode = 'blacklist' | 'whitelist';

export interface ClipboardDisplaySettings {
  density: ClipboardCardDensity;
  preview_lines: number;
  time_format: ClipboardTimeFormat;
  show_char_count: boolean;
  show_byte_size: boolean;
  show_source_app: ClipboardSourceAppDisplay;
  image_max_height: number;
  image_auto_height: boolean;
  drag_indicator: boolean;
}

export interface ClipboardPreviewSettings {
  image_enabled: boolean;
  text_enabled: boolean;
  delay_ms: number;
  zoom_step: number;
  position: ClipboardPreviewPosition;
}

export interface ClipboardShortcutsSettings {
  paste: string;
  plain_paste: string;
  delete: string;
  favorite: string;
  edit: string;
  focus_search: string[];
  close: string;
}

export interface ClipboardNavigationSettings {
  enabled: boolean;
}

export interface ClipboardDataSettings {
  max_items: number;
  retain_days: number;
  max_item_bytes: number;
}

export interface ClipboardAppFilterSettings {
  enabled: boolean;
  mode: ClipboardAppFilterMode;
  patterns: string[];
}

export interface ClipboardSettings {
  enabled: boolean;
  hotkey: string;
  image_copy_hotkey_enabled: boolean;
  image_copy_hotkey: string;
  explorer_context_menu_enabled: boolean;
  max_items: number;
  retain_days: number;
  max_item_bytes: number;
  preview_delay_ms: number;
  enable_text_preview: boolean;
  use_win_v_replacement: boolean;
  run_as_admin: boolean;
  show_startup_notification: boolean;
  dedup_strategy: ClipboardDedupStrategy;
  reinsert_on_self_copy: boolean;
  display: ClipboardDisplaySettings;
  preview: ClipboardPreviewSettings;
  shortcuts: ClipboardShortcutsSettings;
  navigation: ClipboardNavigationSettings;
  data: ClipboardDataSettings;
  app_filter: ClipboardAppFilterSettings;
}

export type DeepPartial<T> = {
  [K in keyof T]?: T[K] extends Array<infer U>
    ? U[]
    : T[K] extends object
      ? DeepPartial<T[K]>
      : T[K];
};

const DEFAULT_CLIPBOARD_SETTINGS: ClipboardSettings = {
  enabled: true,
  hotkey: 'Alt+C',
  image_copy_hotkey_enabled: false,
  image_copy_hotkey: 'Ctrl+Alt+C',
  explorer_context_menu_enabled: false,
  max_items: 1000,
  retain_days: 30,
  max_item_bytes: 10 * 1024 * 1024,
  preview_delay_ms: 500,
  enable_text_preview: false,
  use_win_v_replacement: false,
  run_as_admin: false,
  show_startup_notification: true,
  dedup_strategy: 'move_to_top',
  reinsert_on_self_copy: false,
  display: {
    density: 'standard',
    preview_lines: 3,
    time_format: 'relative',
    show_char_count: true,
    show_byte_size: true,
    show_source_app: 'both',
    image_max_height: 120,
    image_auto_height: true,
    drag_indicator: true,
  },
  preview: {
    image_enabled: true,
    text_enabled: false,
    delay_ms: 500,
    zoom_step: 10,
    position: 'auto',
  },
  shortcuts: {
    paste: 'Enter',
    plain_paste: 'Shift+Enter',
    delete: 'Delete',
    favorite: 'Ctrl+D',
    edit: 'Ctrl+E',
    focus_search: ['Ctrl+F'],
    close: 'Escape',
  },
  navigation: {
    enabled: true,
  },
  data: {
    max_items: 1000,
    retain_days: 30,
    max_item_bytes: 10 * 1024 * 1024,
  },
  app_filter: {
    enabled: false,
    mode: 'blacklist',
    patterns: [],
  },
};

export function cloneClipboardSettings(settings: ClipboardSettings): ClipboardSettings {
  return {
    ...settings,
    display: { ...settings.display },
    preview: { ...settings.preview },
    shortcuts: {
      ...settings.shortcuts,
      focus_search: [...settings.shortcuts.focus_search],
    },
    navigation: { ...settings.navigation },
    data: { ...settings.data },
    app_filter: {
      ...settings.app_filter,
      patterns: [...settings.app_filter.patterns],
    },
  };
}

export function createDefaultClipboardSettings(): ClipboardSettings {
  return cloneClipboardSettings(DEFAULT_CLIPBOARD_SETTINGS);
}

export function normalizeClipboardSettings(
  input: DeepPartial<ClipboardSettings> | null | undefined,
): ClipboardSettings {
  const defaults = createDefaultClipboardSettings();
  const next: ClipboardSettings = {
    enabled: input?.enabled ?? defaults.enabled,
    hotkey: input?.hotkey ?? defaults.hotkey,
    image_copy_hotkey_enabled:
      input?.image_copy_hotkey_enabled ?? defaults.image_copy_hotkey_enabled,
    image_copy_hotkey: input?.image_copy_hotkey ?? defaults.image_copy_hotkey,
    explorer_context_menu_enabled:
      input?.explorer_context_menu_enabled ?? defaults.explorer_context_menu_enabled,
    max_items: input?.max_items ?? defaults.max_items,
    retain_days: input?.retain_days ?? defaults.retain_days,
    max_item_bytes: input?.max_item_bytes ?? defaults.max_item_bytes,
    preview_delay_ms: input?.preview_delay_ms ?? defaults.preview_delay_ms,
    enable_text_preview: input?.enable_text_preview ?? defaults.enable_text_preview,
    use_win_v_replacement: input?.use_win_v_replacement ?? defaults.use_win_v_replacement,
    run_as_admin: input?.run_as_admin ?? defaults.run_as_admin,
    show_startup_notification:
      input?.show_startup_notification ?? defaults.show_startup_notification,
    dedup_strategy: input?.dedup_strategy ?? defaults.dedup_strategy,
    reinsert_on_self_copy: input?.reinsert_on_self_copy ?? defaults.reinsert_on_self_copy,
    display: {
      ...defaults.display,
      ...(input?.display ?? {}),
    },
    preview: {
      ...defaults.preview,
      ...(input?.preview ?? {}),
    },
    shortcuts: {
      ...defaults.shortcuts,
      ...(input?.shortcuts ?? {}),
      focus_search: input?.shortcuts?.focus_search
        ? [...input.shortcuts.focus_search]
        : [...defaults.shortcuts.focus_search],
    },
    navigation: {
      ...defaults.navigation,
      ...(input?.navigation ?? {}),
    },
    data: {
      ...defaults.data,
      ...(input?.data ?? {}),
    },
    app_filter: {
      ...defaults.app_filter,
      ...(input?.app_filter ?? {}),
      patterns: input?.app_filter?.patterns
        ? [...input.app_filter.patterns]
        : [...defaults.app_filter.patterns],
    },
  };

  next.preview.delay_ms = input?.preview?.delay_ms ?? input?.preview_delay_ms ?? next.preview.delay_ms;
  next.preview_delay_ms = next.preview.delay_ms;
  next.preview.text_enabled =
    input?.preview?.text_enabled ?? input?.enable_text_preview ?? next.preview.text_enabled;
  next.enable_text_preview = next.preview.text_enabled;
  next.data.max_items = input?.max_items ?? input?.data?.max_items ?? next.data.max_items;
  next.max_items = next.data.max_items;
  next.data.retain_days = input?.retain_days ?? input?.data?.retain_days ?? next.data.retain_days;
  next.retain_days = next.data.retain_days;
  next.data.max_item_bytes =
    input?.max_item_bytes ?? input?.data?.max_item_bytes ?? next.data.max_item_bytes;
  next.max_item_bytes = next.data.max_item_bytes;

  return next;
}

export interface ClipboardItem {
  id: number;
  kind: ClipboardContentKind;
  content_preview: string;
  content_full: string | null;
  rtf_content: string | null;
  html: string | null;
  image_path: string | null;
  image_width: number | null;
  image_height: number | null;
  file_paths: string[] | null;
  byte_size: number;
  char_count: number;
  hash: string;
  source_app: string | null;
  source_app_icon: string | null;
  from_self: boolean;
  group_id: number | null;
  is_favorite: boolean;
  is_pinned: boolean;
  favorite_sort_index: number | null;
  created_at: number;
  updated_at: number;
}

export interface ClipboardSearchFilters {
  kind?: ClipboardContentKind | null;
  from?: string | null;
  to?: string | null;
  app?: string | null;
  fav?: boolean;
  size_gt?: number | null;
  size_lt?: number | null;
  group_id?: number | null;
  pinned_only?: boolean;
}

export interface ClipboardSearchPayload {
  keywords: string[];
  filters: ClipboardSearchFilters;
}

export interface ClipboardListQuery {
  filter: ClipboardFilter;
  search: string;
  search_payload?: ClipboardSearchPayload | null;
  group_id?: number | null;
  pinned_only?: boolean;
  op_type?: string | null;
  op_from_ms?: number | null;
  op_to_ms?: number | null;
  op_app?: string | null;
  op_fav_only?: boolean;
  op_size_gt?: number | null;
  op_size_lt?: number | null;
  offset: number;
  limit: number;
}

export interface ClipboardListResult {
  items: ClipboardItem[];
  total: number;
}

export interface ClipboardStats {
  total: number;
  db_bytes: number;
  image_count: number;
  images_bytes: number;
}

export interface ClipboardGroup {
  id: number;
  name: string;
  sort_index: number;
  created_at: number;
  item_count: number;
}

export interface FilePathStatus {
  path: string;
  exists: boolean;
  size: number | null;
}
