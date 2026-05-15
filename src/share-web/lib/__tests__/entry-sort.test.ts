import { describe, expect, it } from 'vitest';

import { sortEntriesForExplorer } from '../entry-sort';
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
  const isDir = overrides.is_dir ?? (overrides.kind === 'directory' || overrides.kind === 'share_root');
  const name = overrides.name ?? 'entry';

  return {
    node_id: overrides.node_id ?? name,
    parent_id: overrides.parent_id ?? null,
    kind: overrides.kind ?? (isDir ? 'directory' : 'file'),
    name,
    root_id: overrides.root_id ?? 'root-1',
    root_alias: overrides.root_alias ?? 'UMS_TEMP',
    relative_path: overrides.relative_path ?? name,
    display_path: overrides.display_path ?? name,
    is_dir: isDir,
    size: overrides.size ?? null,
    modified: overrides.modified ?? null,
    permissions: overrides.permissions ?? basePermissions,
  };
}

const entries = [
  node({ name: 'Folder 10', is_dir: true, modified: '2026-05-01 09:00' }),
  node({ name: 'alpha.txt', is_dir: false, size: 10, modified: '2026-05-15 09:00' }),
  node({ name: 'Folder 2', is_dir: true, modified: '2026-05-14 09:00' }),
  node({ name: 'beta.txt', is_dir: false, size: 20, modified: '2026-05-10 09:00' }),
];

describe('sortEntriesForExplorer', () => {
  it('puts folders first for ascending name sort and uses natural name order inside each group', () => {
    expect(sortEntriesForExplorer(entries, { key: 'name', direction: 'asc' }).map((entry) => entry.name))
      .toEqual(['Folder 2', 'Folder 10', 'alpha.txt', 'beta.txt']);
  });

  it('puts files first for descending name sort and reverses natural name order inside each group', () => {
    expect(sortEntriesForExplorer(entries, { key: 'name', direction: 'desc' }).map((entry) => entry.name))
      .toEqual(['beta.txt', 'alpha.txt', 'Folder 10', 'Folder 2']);
  });

  it('puts files first for descending size and modified sorts', () => {
    expect(sortEntriesForExplorer(entries, { key: 'size', direction: 'desc' }).map((entry) => entry.name))
      .toEqual(['beta.txt', 'alpha.txt', 'Folder 10', 'Folder 2']);

    expect(sortEntriesForExplorer(entries, { key: 'modified', direction: 'desc' }).map((entry) => entry.name))
      .toEqual(['alpha.txt', 'beta.txt', 'Folder 2', 'Folder 10']);
  });

  it('does not mutate the source list', () => {
    const originalNames = entries.map((entry) => entry.name);

    sortEntriesForExplorer(entries, { key: 'modified', direction: 'desc' });

    expect(entries.map((entry) => entry.name)).toEqual(originalNames);
  });
});
