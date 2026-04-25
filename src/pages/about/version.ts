export interface HasVersion {
  version: string;
}

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)/;

function parseSemver(value: string): [number, number, number] | null {
  const match = SEMVER_RE.exec(value.trim());
  if (!match) {
    return null;
  }
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

export function compareVersionsAsc<A extends HasVersion, B extends HasVersion>(a: A, b: B): number {
  const left = parseSemver(a.version);
  const right = parseSemver(b.version);
  if (!left && !right) {
    return 0;
  }
  if (!left) {
    return -1;
  }
  if (!right) {
    return 1;
  }

  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) {
      return left[index] - right[index];
    }
  }
  return 0;
}

export function formatReleaseDate(value: string): string {
  if (!value) {
    return '';
  }
  return value.replace(/-/g, '.');
}

export function isCurrentVersion(candidate: string, current: string): boolean {
  return candidate.trim() === current.trim();
}
