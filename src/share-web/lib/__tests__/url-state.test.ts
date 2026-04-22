import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { fileShareApi } from '../../api';
import {
  parseHash,
  pushPath,
  replacePath,
  serialize,
  subscribe,
} from '../url-state';

describe('parseHash', () => {
  it('returns home for empty hash', () => {
    expect(parseHash('')).toEqual({ segments: [], q: '', scope: 'global' });
  });

  it('returns home for #/', () => {
    expect(parseHash('#/')).toEqual({ segments: [], q: '', scope: 'global' });
  });

  it('parses nested segments', () => {
    expect(parseHash('#/UMS_TEMP/sub')).toEqual({
      segments: ['UMS_TEMP', 'sub'],
      q: '',
      scope: 'current',
    });
  });

  it('skips empty path segments', () => {
    expect(parseHash('#/UMS_TEMP//sub')).toEqual({
      segments: ['UMS_TEMP', 'sub'],
      q: '',
      scope: 'current',
    });
  });

  it('parses home search from #?q=foo', () => {
    expect(parseHash('#?q=foo')).toEqual({
      segments: [],
      q: 'foo',
      scope: 'global',
    });
  });

  it('parses query keyword and scope', () => {
    expect(parseHash('#/UMS_TEMP?q=foo&scope=current')).toEqual({
      segments: ['UMS_TEMP'],
      q: 'foo',
      scope: 'current',
    });
  });

  it('falls back scope when value is invalid', () => {
    expect(parseHash('#/?scope=invalid')).toEqual({
      segments: [],
      q: '',
      scope: 'global',
    });
  });

  it('decodes percent-encoded chinese segments', () => {
    expect(parseHash(`#/${encodeURIComponent('中文目录')}`)).toEqual({
      segments: ['中文目录'],
      q: '',
      scope: 'current',
    });
  });

  it('treats encoded slash inside a segment as invalid input', () => {
    expect(parseHash('#/foo%2Fbar')).toEqual({
      segments: [],
      q: '',
      scope: 'global',
    });
  });
});

describe('serialize', () => {
  it('returns empty for home with no search', () => {
    expect(serialize({ segments: [], q: '', scope: 'global' })).toBe('');
  });

  it('serializes home search without a leading slash', () => {
    expect(serialize({ segments: [], q: 'foo', scope: 'global' })).toBe('?q=foo&scope=global');
  });

  it('round-trips with parseHash', () => {
    const state = {
      segments: ['UMS_TEMP', '中文', 'leaf'],
      q: '关键字',
      scope: 'current' as const,
    };

    expect(parseHash(`#${serialize(state)}`)).toEqual(state);
  });

  it('percent-encodes chinese segments', () => {
    expect(
      serialize({ segments: ['中文'], q: '', scope: 'current' }),
    ).toBe(`/${encodeURIComponent('中文')}`);
  });
});

describe('subscribe', () => {
  beforeEach(() => {
    history.replaceState(null, '', '#');
  });

  it('ignores self-written hash changes', () => {
    const calls: string[] = [];
    const off = subscribe((state) => {
      calls.push(state.segments.join('/'));
    });

    pushPath({ segments: ['UMS_TEMP', 'sub'], q: '', scope: 'current' });
    window.dispatchEvent(new HashChangeEvent('hashchange'));

    expect(calls).toEqual([]);
    off();
  });

  it('fires for external hash changes', () => {
    const calls: Array<{ segments: string[]; q: string; scope: string }> = [];
    const off = subscribe((state) => {
      calls.push(state);
    });

    history.replaceState(null, '', '#/UMS_TEMP/sub');
    window.dispatchEvent(new HashChangeEvent('hashchange'));

    expect(calls).toEqual([
      { segments: ['UMS_TEMP', 'sub'], q: '', scope: 'current' },
    ]);
    off();
  });

  it('fires for external popstate events', () => {
    const calls: Array<{ segments: string[]; q: string; scope: string }> = [];
    const off = subscribe((state) => {
      calls.push(state);
    });

    history.replaceState(null, '', '#/UMS_TEMP/sub');
    window.dispatchEvent(new PopStateEvent('popstate'));

    expect(calls).toEqual([
      { segments: ['UMS_TEMP', 'sub'], q: '', scope: 'current' },
    ]);
    off();
  });

  it('deduplicates a single navigation reported by popstate and hashchange', () => {
    const calls: Array<{ segments: string[]; q: string; scope: string }> = [];
    const off = subscribe((state) => {
      calls.push(state);
    });

    history.replaceState(null, '', '#/UMS_TEMP/sub');
    window.dispatchEvent(new PopStateEvent('popstate'));
    window.dispatchEvent(new HashChangeEvent('hashchange'));

    expect(calls).toEqual([
      { segments: ['UMS_TEMP', 'sub'], q: '', scope: 'current' },
    ]);
    off();
  });

  it('suppresses an already-caught-up hash when initialHash is provided', () => {
    const calls: Array<{ segments: string[]; q: string; scope: string }> = [];

    history.replaceState(null, '', '#/UMS_TEMP/sub');
    const off = subscribe((state) => {
      calls.push(state);
    }, {
      initialHash: window.location.hash,
    });

    window.dispatchEvent(new HashChangeEvent('hashchange'));

    expect(calls).toEqual([]);
    off();
  });

  it('does not increase history length when replacing', () => {
    const before = history.length;

    replacePath({ segments: ['UMS_TEMP'], q: '', scope: 'current' });

    expect(history.length).toBe(before);
  });
});

describe('resolvePath', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('omits the query string for empty segments', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      node_id: null,
      kind: 'home',
      canonical_segments: [],
    }), {
      status: 200,
      headers: {
        'Content-Type': 'application/json',
      },
    }));

    vi.stubGlobal('fetch', fetchMock);

    await fileShareApi.resolvePath([]);

    expect(fetchMock).toHaveBeenCalledWith('/api/resolve', expect.objectContaining({
      credentials: 'include',
    }));
  });

  it('encodes non-empty segments into the path query', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      node_id: 'node-1',
      kind: 'directory',
      canonical_segments: ['中文', 'sub'],
    }), {
      status: 200,
      headers: {
        'Content-Type': 'application/json',
      },
    }));

    vi.stubGlobal('fetch', fetchMock);

    await fileShareApi.resolvePath(['中文', 'sub']);

    expect(fetchMock).toHaveBeenCalledWith(
      `/api/resolve?path=${encodeURIComponent('中文')}/${encodeURIComponent('sub')}`,
      expect.objectContaining({
        credentials: 'include',
      }),
    );
  });
});
