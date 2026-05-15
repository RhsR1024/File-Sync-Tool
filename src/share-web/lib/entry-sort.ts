import type { FileShareNode } from '../types';
import type { EntrySortPreference } from './sort-preference';

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

function directionFactor(direction: EntrySortPreference['direction']): number {
  return direction === 'asc' ? 1 : -1;
}

function compareEntryGroup(a: FileShareNode, b: FileShareNode, direction: EntrySortPreference['direction']): number {
  if (a.is_dir === b.is_dir) {
    return 0;
  }
  if (direction === 'asc') {
    return a.is_dir ? -1 : 1;
  }
  return a.is_dir ? 1 : -1;
}

function parsedModified(entry: FileShareNode): number {
  if (!entry.modified) {
    return 0;
  }
  const ms = Date.parse(entry.modified);
  return Number.isFinite(ms) ? ms : 0;
}

function compareBySortKey(a: FileShareNode, b: FileShareNode, preference: EntrySortPreference): number {
  if (preference.key === 'name') {
    return collator.compare(a.name, b.name);
  }
  if (preference.key === 'size') {
    return (a.size ?? -1) - (b.size ?? -1);
  }
  return parsedModified(a) - parsedModified(b);
}

export function sortEntriesForExplorer(
  entries: FileShareNode[],
  preference: EntrySortPreference,
): FileShareNode[] {
  const factor = directionFactor(preference.direction);
  return entries.slice().sort((a, b) => {
    const groupCompare = compareEntryGroup(a, b, preference.direction);
    if (groupCompare !== 0) {
      return groupCompare;
    }

    const keyCompare = compareBySortKey(a, b, preference);
    if (keyCompare !== 0) {
      return keyCompare * factor;
    }

    return collator.compare(a.name, b.name) * factor;
  });
}
