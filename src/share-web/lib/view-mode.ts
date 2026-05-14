const STORAGE_KEY = 'share-web:view';

export type EntryViewMode = 'list' | 'grid';

const DEFAULT_VIEW: EntryViewMode = 'list';

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

export function loadViewMode(): EntryViewMode {
  const storage = safeStorage();
  if (!storage) {
    return DEFAULT_VIEW;
  }
  const raw = storage.getItem(STORAGE_KEY);
  return raw === 'grid' ? 'grid' : DEFAULT_VIEW;
}

export function saveViewMode(mode: EntryViewMode): void {
  const storage = safeStorage();
  if (!storage) {
    return;
  }
  try {
    storage.setItem(STORAGE_KEY, mode);
  } catch {
    /* Quota or privacy mode — non-fatal. */
  }
}
