export type BatchStatus = 'ok' | 'duplicate_in_batch' | 'invalid_path';

export interface BatchEntryResolution {
  rawSource: string;
  normalizedSegments: string[];
  tail: string;
  disambiguatorSegments: string[];
  effectiveTargetRoot: string;
  finalTarget: string;
  status: BatchStatus;
}

const SEGMENT_SPLIT = /[\\/]+/;

function normalizeSegments(raw: string): string[] {
  const trimmed = raw.replace(/^\s+|\s+$/g, '').replace(/[\\/]+$/g, '');
  if (!trimmed) return [];
  // Preserve UNC prefix `\\server\share` semantics: keep the first two
  // segments as-is. The split will already produce them as separate items.
  return trimmed.split(SEGMENT_SPLIT).filter((s) => s.length > 0);
}

function joinWindowsPath(parts: string[]): string {
  return parts.join('\\');
}

function buildKey(segs: string[], depth: number): string {
  const start = Math.max(0, segs.length - depth - 1);
  return segs.slice(start).join('/').toLowerCase();
}

export function resolveBatchTargets(
  rawSources: string[],
  targetRoot: string,
): BatchEntryResolution[] {
  const normalized = rawSources.map((raw) => ({
    raw,
    segs: normalizeSegments(raw),
  }));

  const resolutions: BatchEntryResolution[] = normalized.map(({ raw, segs }) => ({
    rawSource: raw,
    normalizedSegments: segs,
    tail: segs[segs.length - 1] ?? '',
    disambiguatorSegments: [],
    effectiveTargetRoot: '',
    finalTarget: '',
    status: segs.length === 0 ? 'invalid_path' : 'ok',
  }));

  const validIndices = resolutions
    .map((r, i) => (r.status === 'ok' ? i : -1))
    .filter((i) => i >= 0);

  if (validIndices.length === 0) return resolutions;

  const maxSegs = Math.max(...validIndices.map((i) => normalized[i].segs.length));

  for (let depth = 0; depth < maxSegs; depth++) {
    const keys = validIndices.map((i) => buildKey(normalized[i].segs, depth));
    const seen = new Map<string, number>();
    let collided = false;
    for (let k = 0; k < keys.length; k++) {
      if (seen.has(keys[k])) {
        collided = true;
        break;
      }
      seen.set(keys[k], k);
    }
    if (!collided) {
      for (let k = 0; k < validIndices.length; k++) {
        const i = validIndices[k];
        const segs = normalized[i].segs;
        const start = Math.max(0, segs.length - depth - 1);
        const disambig = segs.slice(start, segs.length - 1);
        const tail = segs[segs.length - 1];
        const effRoot = joinWindowsPath([targetRoot.replace(/[\\/]+$/g, ''), ...disambig]);
        resolutions[i].disambiguatorSegments = disambig;
        resolutions[i].effectiveTargetRoot = effRoot;
        resolutions[i].finalTarget = joinWindowsPath([effRoot, tail]);
      }
      return resolutions;
    }
  }

  // Could not disambiguate at max depth → mark colliding entries.
  const finalKeys = validIndices.map((i) =>
    buildKey(normalized[i].segs, normalized[i].segs.length - 1),
  );
  const groups = new Map<string, number[]>();
  finalKeys.forEach((key, k) => {
    const arr = groups.get(key) ?? [];
    arr.push(validIndices[k]);
    groups.set(key, arr);
  });
  for (const [, members] of groups) {
    if (members.length > 1) {
      for (const i of members) resolutions[i].status = 'duplicate_in_batch';
    }
  }
  // Non-duplicate entries (singletons) at max depth still need values; resolve them.
  for (const i of validIndices) {
    if (resolutions[i].status === 'duplicate_in_batch') continue;
    const segs = normalized[i].segs;
    const disambig = segs.slice(0, segs.length - 1);
    const tail = segs[segs.length - 1];
    const effRoot = joinWindowsPath([targetRoot.replace(/[\\/]+$/g, ''), ...disambig]);
    resolutions[i].disambiguatorSegments = disambig;
    resolutions[i].effectiveTargetRoot = effRoot;
    resolutions[i].finalTarget = joinWindowsPath([effRoot, tail]);
  }
  return resolutions;
}
