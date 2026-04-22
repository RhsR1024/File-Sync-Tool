export type SearchScope = 'global' | 'current';

export interface UrlState {
  segments: string[];
  q: string;
  scope: SearchScope;
}

let lastWrittenHash: string | null = null;

function cloneHomeState(): UrlState {
  return {
    segments: [],
    q: '',
    scope: 'global',
  };
}

function isSearchScope(value: string | null): value is SearchScope {
  return value === 'global' || value === 'current';
}

function defaultScope(segments: string[]): SearchScope {
  return segments.length > 0 ? 'current' : 'global';
}

function normalizeWrittenHash(hash: string): string {
  return hash === '' ? '#' : hash;
}

function normalizeLocationHash(hash: string): string {
  return hash === '' ? '#' : hash;
}

function writeHash(nextHash: string, method: 'pushState' | 'replaceState'): void {
  if (typeof window === 'undefined') {
    return;
  }

  const normalizedHash = normalizeWrittenHash(nextHash);

  lastWrittenHash = normalizedHash;

  if (method === 'pushState') {
    window.history.pushState(null, '', normalizedHash);
  } else {
    window.history.replaceState(null, '', normalizedHash);
  }

  queueMicrotask(() => {
    if (lastWrittenHash === normalizedHash) {
      lastWrittenHash = null;
    }
  });
}

export function parseHash(hash: string = typeof location !== 'undefined' ? location.hash : ''): UrlState {
  if (!hash || hash === '#' || hash === '#/') {
    return cloneHomeState();
  }

  const stripped = hash.startsWith('#') ? hash.slice(1) : hash;
  const normalized = stripped.startsWith('/') || stripped.startsWith('?')
    ? stripped
    : `/${stripped}`;

  const queryIndex = normalized.indexOf('?');
  const pathPart = queryIndex >= 0 ? normalized.slice(0, queryIndex) : normalized;
  const queryPart = queryIndex >= 0 ? normalized.slice(queryIndex + 1) : '';

  let segments: string[];
  try {
    segments = pathPart
      .split('/')
      .filter((piece) => piece.length > 0)
      .map((piece) => decodeURIComponent(piece));
  } catch {
    return cloneHomeState();
  }

  if (segments.some((segment) => segment.includes('/'))) {
    return cloneHomeState();
  }

  const params = new URLSearchParams(queryPart);
  const q = params.get('q') ?? '';
  const rawScope = params.get('scope');
  const scope = q.length > 0 && isSearchScope(rawScope)
    ? rawScope
    : defaultScope(segments);

  return {
    segments,
    q,
    scope,
  };
}

export function serialize(state: UrlState): string {
  const hasSegments = state.segments.length > 0;
  const hasSearch = state.q.length > 0;

  if (!hasSegments && !hasSearch) {
    return '';
  }

  if (!hasSearch) {
    return `/${state.segments.map((segment) => encodeURIComponent(segment)).join('/')}`;
  }

  const params = new URLSearchParams();
  params.set('q', state.q);
  params.set('scope', state.scope);
  const query = params.toString();

  if (!hasSegments) {
    return `?${query}`;
  }

  return `/${state.segments.map((segment) => encodeURIComponent(segment)).join('/')}?${query}`;
}

export function pushPath(state: UrlState): void {
  writeHash(`#${serialize(state)}`, 'pushState');
}

export function replacePath(state: UrlState): void {
  writeHash(`#${serialize(state)}`, 'replaceState');
}

export function subscribe(
  cb: (state: UrlState) => void,
  options: { initialHash?: string } = {},
): () => void {
  if (typeof window === 'undefined') {
    return () => {};
  }

  let lastNotifiedHash: string | null = options.initialHash
    ? normalizeLocationHash(options.initialHash)
    : null;

  const handler = () => {
    const currentHash = normalizeLocationHash(window.location.hash);

    if (lastWrittenHash !== null && currentHash === lastWrittenHash) {
      return;
    }

    if (lastNotifiedHash === currentHash) {
      return;
    }

    lastNotifiedHash = currentHash;
    cb(parseHash(currentHash));
  };

  window.addEventListener('hashchange', handler);
  window.addEventListener('popstate', handler);

  return () => {
    window.removeEventListener('hashchange', handler);
    window.removeEventListener('popstate', handler);
  };
}
