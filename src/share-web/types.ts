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

export interface FileShareFeatureFlags {
  image_preview_enabled: boolean;
  thumbnail_enabled: boolean;
}

export interface FileShareSession {
  account_id: string;
  account_name: string;
  is_guest: boolean;
  permissions: FileSharePermissionSet;
  features: FileShareFeatureFlags;
}

export type FileShareNodeKind = 'share_root' | 'directory' | 'file';
export type FileShareTreeCurrentKind = 'home' | 'share_root' | 'directory';
export type FileShareSearchScope = 'current' | 'global';

export interface FileShareNode {
  node_id: string;
  parent_id: string | null;
  kind: FileShareNodeKind;
  name: string;
  root_id: string;
  root_alias: string;
  relative_path: string;
  display_path: string;
  is_dir: boolean;
  size: number | null;
  modified: string | null;
  permissions: FileSharePermissionSet;
}

export interface FileShareTreeCurrent {
  node_id: string | null;
  name: string;
  kind: FileShareTreeCurrentKind;
}

export interface FileShareBreadcrumb {
  node_id: string | null;
  label: string;
}

export interface FileShareTreeResponse {
  current: FileShareTreeCurrent;
  breadcrumbs: FileShareBreadcrumb[];
  children: FileShareNode[];
}

export interface FileShareSearchResponse {
  scope: 'global' | 'subtree';
  results: FileShareNode[];
}

export type FileShareAction = 'upload' | 'rename' | 'delete' | 'preview' | 'searchGlobal';

export function shouldPromptForAccountSwitch(session: FileShareSession | null): boolean {
  return Boolean(session?.is_guest);
}

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

export function formatFileSize(size: number | null): string {
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

export function canPreviewEntry(
  session: FileShareSession | null | undefined,
  entry: FileShareNode,
): boolean {
  return Boolean(
    session?.features.image_preview_enabled
    && !entry.is_dir
    && entry.permissions.preview_image
    && isImageEntry(entry.name),
  );
}

export function canRenderEntryThumbnail(
  session: FileShareSession | null | undefined,
  entry: FileShareNode,
): boolean {
  return Boolean(session?.features.thumbnail_enabled) && canPreviewEntry(session, entry);
}

export function isHomeNode(kind: FileShareTreeCurrentKind | null | undefined): boolean {
  return kind === 'home';
}

export function isNodeWritable(
  permissions: FileSharePermissionSet | null | undefined,
): boolean {
  return Boolean(
    permissions
    && (
      permissions.upload_file
      || permissions.upload_directory
      || permissions.create_directory
      || permissions.create_text
      || permissions.rename
      || permissions.delete
    ),
  );
}
