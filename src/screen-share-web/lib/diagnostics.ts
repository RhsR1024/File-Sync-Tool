export const SCREEN_SHARE_DIAGNOSTICS_GLOBAL = '__SCREEN_SHARE_DIAGNOSTICS__' as const;

export interface ScreenShareDiagnosticsSnapshot {
  capturedAtUnixMs: number;
  transport: string | null;
  server: Record<string, unknown>;
  client: Record<string, unknown>;
}

export interface ScreenShareDiagnosticsApi {
  /** Read-only local snapshot. It never uploads or persists screen/input data. */
  snapshot(): ScreenShareDiagnosticsSnapshot;
}

declare global {
  interface Window {
    __SCREEN_SHARE_DIAGNOSTICS__?: ScreenShareDiagnosticsApi;
  }
}

function cloneAndFreeze(value: unknown, seen = new WeakMap<object, unknown>()): unknown {
  if (value === null || typeof value !== 'object') return value;
  const existing = seen.get(value);
  if (existing !== undefined) return existing;
  if (Array.isArray(value)) {
    const clone: unknown[] = [];
    seen.set(value, clone);
    for (const item of value) clone.push(cloneAndFreeze(item, seen));
    return Object.freeze(clone);
  }
  const clone: Record<string, unknown> = {};
  seen.set(value, clone);
  for (const [key, item] of Object.entries(value as Record<string, unknown>)) {
    clone[key] = cloneAndFreeze(item, seen);
  }
  return Object.freeze(clone);
}

export function installScreenShareDiagnostics(
  provider: () => ScreenShareDiagnosticsSnapshot,
): () => void {
  const api = Object.freeze<ScreenShareDiagnosticsApi>({
    snapshot: () => cloneAndFreeze(provider()) as ScreenShareDiagnosticsSnapshot,
  });
  window[SCREEN_SHARE_DIAGNOSTICS_GLOBAL] = api;
  return () => {
    if (window[SCREEN_SHARE_DIAGNOSTICS_GLOBAL] === api) {
      delete window[SCREEN_SHARE_DIAGNOSTICS_GLOBAL];
    }
  };
}
