import { describe, expect, it } from 'vitest';

import { entryListHint } from '../entry-display';
import type { FileShareNode } from '../../types';

const basePermissions = {
  browse: true,
  download_file: true,
  download_archive: true,
  upload_file: false,
  upload_directory: false,
  create_directory: false,
  create_text: false,
  rename: false,
  delete: false,
  preview_image: false,
  search_current: true,
  search_global: true,
};

function node(overrides: Partial<FileShareNode>): FileShareNode {
  return {
    node_id: overrides.node_id ?? 'node-1',
    parent_id: overrides.parent_id ?? null,
    kind: overrides.kind ?? 'file',
    name: overrides.name ?? 'report.txt',
    root_id: overrides.root_id ?? 'root-1',
    root_alias: overrides.root_alias ?? 'UMS_TEMP',
    relative_path: overrides.relative_path ?? 'report.txt',
    display_path: overrides.display_path ?? 'UMS_TEMP/report.txt',
    is_dir: overrides.is_dir ?? false,
    size: overrides.size ?? 120,
    modified: overrides.modified ?? '2026-05-15 09:00',
    permissions: overrides.permissions ?? basePermissions,
  };
}

describe('entryListHint', () => {
  it('hides root aliases and file type labels during normal browsing', () => {
    expect(entryListHint(node({
      kind: 'directory',
      is_dir: true,
      name: '260413',
      root_alias: 'UMS_TEMP',
      display_path: 'UMS_TEMP/260413',
    }), false)).toBe('');

    expect(entryListHint(node({
      name: 'file-sync-tool-1.1.1-202605151547.exe',
      display_path: 'UMS_TEMP/file-sync-tool-1.1.1-202605151547.exe',
    }), false)).toBe('');
  });

  it('keeps a path hint for search results outside the current display name', () => {
    expect(entryListHint(node({
      name: 'report.txt',
      display_path: 'UMS_TEMP/archive/report.txt',
    }), true)).toBe('UMS_TEMP/archive/report.txt');
  });
});
