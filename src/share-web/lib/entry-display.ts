import type { FileShareNode } from '../types';

export function entryListHint(entry: FileShareNode, searchActive: boolean): string {
  if (searchActive && entry.display_path && entry.display_path !== entry.name) {
    return entry.display_path;
  }
  return '';
}
