export type ClipboardContentKind = 'text' | 'html' | 'image' | 'file';
export type ClipboardFilter = 'all' | 'text' | 'image' | 'file' | 'favorite';

export interface ClipboardItem {
  id: number;
  kind: ClipboardContentKind;
  content_preview: string;
  content_full: string | null;
  html: string | null;
  image_path: string | null;
  image_width: number | null;
  image_height: number | null;
  file_paths: string[] | null;
  byte_size: number;
  hash: string;
  source_app: string | null;
  is_favorite: boolean;
  favorite_sort_index: number | null;
  created_at: number;
  updated_at: number;
}

export interface ClipboardSettings {
  enabled: boolean;
  hotkey: string;
  max_items: number;
  retain_days: number;
  max_item_bytes: number;
  preview_delay_ms: number;
  enable_text_preview: boolean;
  use_win_v_replacement: boolean;
  run_as_admin: boolean;
  show_startup_notification: boolean;
}

export interface ClipboardListQuery {
  filter: ClipboardFilter;
  search: string;
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
