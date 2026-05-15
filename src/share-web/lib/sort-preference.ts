const STORAGE_KEY = 'share-web:sort';

export type EntrySortKey = 'name' | 'size' | 'modified';
export type EntrySortDirection = 'asc' | 'desc';

export interface EntrySortPreference {
  key: EntrySortKey;
  direction: EntrySortDirection;
}

const DEFAULT_PREFERENCE: EntrySortPreference = {
  key: 'modified',
  direction: 'desc',
};

function safeStorage(): Storage | null {
  try {
    if (typeof window === 'undefined') {
      return null;
    }
    return window.localStorage;
  } catch {
    return null;
  }
}

function isSortKey(value: unknown): value is EntrySortKey {
  return value === 'name' || value === 'size' || value === 'modified';
}

function isSortDirection(value: unknown): value is EntrySortDirection {
  return value === 'asc' || value === 'desc';
}

export function loadSortPreference(): EntrySortPreference {
  const storage = safeStorage();
  if (!storage) {
    return { ...DEFAULT_PREFERENCE };
  }
  const raw = storage.getItem(STORAGE_KEY);
  if (!raw) {
    return { ...DEFAULT_PREFERENCE };
  }
  try {
    const parsed = JSON.parse(raw) as Partial<EntrySortPreference>;
    return {
      key: isSortKey(parsed?.key) ? parsed.key : DEFAULT_PREFERENCE.key,
      direction: isSortDirection(parsed?.direction) ? parsed.direction : DEFAULT_PREFERENCE.direction,
    };
  } catch {
    return { ...DEFAULT_PREFERENCE };
  }
}

export function saveSortPreference(preference: EntrySortPreference): void {
  const storage = safeStorage();
  if (!storage) {
    return;
  }
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(preference));
  } catch {
    /* Quota or privacy mode — non-fatal. */
  }
}
