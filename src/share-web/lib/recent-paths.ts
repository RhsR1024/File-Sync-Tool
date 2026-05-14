const STORAGE_KEY = 'share-web:recent-paths';
const MAX_ITEMS = 6;

export interface RecentPathEntry {
  node_id: string;
  label: string;
  /** Last-visited timestamp (ms). */
  visited_at: number;
}

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

export function loadRecentPaths(): RecentPathEntry[] {
  const storage = safeStorage();
  if (!storage) {
    return [];
  }
  const raw = storage.getItem(STORAGE_KEY);
  if (!raw) {
    return [];
  }
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed
      .filter((item): item is RecentPathEntry => (
        Boolean(item)
        && typeof (item as RecentPathEntry).node_id === 'string'
        && typeof (item as RecentPathEntry).label === 'string'
        && typeof (item as RecentPathEntry).visited_at === 'number'
      ))
      .slice(0, MAX_ITEMS);
  } catch {
    return [];
  }
}

export function recordRecentPath(entry: Omit<RecentPathEntry, 'visited_at'>): RecentPathEntry[] {
  const storage = safeStorage();
  const existing = loadRecentPaths().filter((item) => item.node_id !== entry.node_id);
  const next: RecentPathEntry[] = [
    {
      ...entry,
      visited_at: Date.now(),
    },
    ...existing,
  ].slice(0, MAX_ITEMS);

  if (storage) {
    try {
      storage.setItem(STORAGE_KEY, JSON.stringify(next));
    } catch {
      /* Quota or privacy mode — non-fatal. */
    }
  }
  return next;
}
