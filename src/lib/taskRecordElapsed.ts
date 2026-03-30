export type TaskElapsedPhase =
  | 'queued'
  | 'copying'
  | 'paused'
  | 'remote_pushing'
  | 'remote_deploying'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

export interface TaskElapsedSnapshot {
  phase: TaskElapsedPhase;
  startedAtMs?: number;
  updatedAt?: number;
  finishedAtMs?: number;
  elapsedSeconds?: number;
}

const toFiniteNumber = (value: unknown) =>
  typeof value === 'number' && Number.isFinite(value) ? value : 0;

export function deriveTaskRecordElapsedSeconds(
  snapshot: TaskElapsedSnapshot,
  liveElapsedSeconds = 0,
): number {
  const persistedElapsed = Math.max(0, Math.floor(toFiniteNumber(snapshot.elapsedSeconds)));
  const startedAtMs = toFiniteNumber(snapshot.startedAtMs);
  const updatedAt = toFiniteNumber(snapshot.updatedAt);
  const finishedAtMs = toFiniteNumber(snapshot.finishedAtMs);
  const liveElapsed = Math.max(0, Math.floor(toFiniteNumber(liveElapsedSeconds)));

  let derivedElapsed = 0;
  if (startedAtMs > 0) {
    if (finishedAtMs >= startedAtMs) {
      derivedElapsed = Math.floor((finishedAtMs - startedAtMs) / 1000);
    } else if (snapshot.phase !== 'queued' && updatedAt >= startedAtMs) {
      derivedElapsed = Math.floor((updatedAt - startedAtMs) / 1000);
    }
  }

  return Math.max(persistedElapsed, derivedElapsed, liveElapsed);
}
