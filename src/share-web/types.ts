export interface FileSharePermissionSet {
  browse: boolean;
  download_file: boolean;
  download_archive: boolean;
  upload_file: boolean;
  upload_directory: boolean;
  create_directory: boolean;
  create_text: boolean;
  rename: boolean;
  delete: boolean;
  preview_image: boolean;
  search_current: boolean;
  search_global: boolean;
}

export interface FileShareSession {
  account_id: string;
  account_name: string;
  is_guest: boolean;
  permissions: FileSharePermissionSet;
}

export interface FileShareRootSummary {
  alias: string;
  path: string;
}

export interface FileShareEntry {
  name: string;
  relative_path: string;
  is_dir: boolean;
  size: number;
  modified: string;
}

export interface FileShareListResponse {
  root_id: string;
  root_alias: string;
  path: string;
  entries: FileShareEntry[];
}

export interface FileShareSearchResult extends FileShareEntry {
  root_id: string;
  root_alias: string;
}

export interface FileShareDisplayEntry extends FileShareEntry {
  root_alias: string;
}

export type FileShareSearchScope = 'current' | 'global';

export type FileShareAction = 'upload' | 'rename' | 'delete' | 'preview' | 'searchGlobal';

export function canRenderAction(
  permissions: FileSharePermissionSet,
  action: FileShareAction,
): boolean {
  switch (action) {
    case 'upload':
      return permissions.upload_file || permissions.upload_directory;
    case 'rename':
      return permissions.rename;
    case 'delete':
      return permissions.delete;
    case 'preview':
      return permissions.preview_image;
    case 'searchGlobal':
      return permissions.search_global;
    default:
      return false;
  }
}

export function formatFileSize(size: number): string {
  if (!size) {
    return '-';
  }
  if (size < 1024) {
    return `${size} B`;
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`;
  }
  if (size < 1024 * 1024 * 1024) {
    return `${(size / 1024 / 1024).toFixed(1)} MB`;
  }
  return `${(size / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function isImageEntry(name: string): boolean {
  return /\.(png|jpe?g|gif|webp|bmp)$/i.test(name);
}

export function joinRelativePath(parent: string, child: string): string {
  const left = parent.trim().replace(/^\/+|\/+$/g, '');
  const right = child.trim().replace(/^\/+|\/+$/g, '');
  if (!left) {
    return right;
  }
  if (!right) {
    return left;
  }
  return `${left}/${right}`;
}

export function parentRelativePath(path: string): string {
  const normalized = path.trim().replace(/^\/+|\/+$/g, '');
  if (!normalized) {
    return '';
  }
  const parts = normalized.split('/');
  parts.pop();
  return parts.join('/');
}

export function splitPathSegments(path: string): string[] {
  return path
    .split('/')
    .map((segment) => segment.trim())
    .filter(Boolean);
}

export function entryToDisplayEntry(
  entry: FileShareEntry,
  rootAlias: string,
): FileShareDisplayEntry {
  return {
    ...entry,
    root_alias: rootAlias,
  };
}
