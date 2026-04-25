export type SingleResult =
  | { ok: true; code: number }
  | { ok: false; error: 'invalid_single' };

export type RangeResult =
  | { ok: true; start: number; end: number }
  | { ok: false; error: 'invalid_range_format' | 'range_reversed' | 'range_too_large' };

export type KeywordResult =
  | { ok: true; keyword: string }
  | { ok: false; error: 'invalid_keyword' };

export const MAX_RANGE_SPAN = 1000;
export const MAX_KEYWORD_LEN = 50;

const DECIMAL_RE = /^\d+$/;

export function parseSingle(raw: string): SingleResult {
  const trimmed = raw.trim();
  if (!DECIMAL_RE.test(trimmed)) {
    return { ok: false, error: 'invalid_single' };
  }

  const code = Number(trimmed);
  if (!Number.isInteger(code) || code < 0) {
    return { ok: false, error: 'invalid_single' };
  }

  return { ok: true, code };
}

export function parseRange(raw: string): RangeResult {
  const trimmed = raw.trim();
  const dashIndex = trimmed.indexOf('-');
  if (dashIndex <= 0 || dashIndex === trimmed.length - 1) {
    return { ok: false, error: 'invalid_range_format' };
  }

  const startStr = trimmed.slice(0, dashIndex).trim();
  const endStr = trimmed.slice(dashIndex + 1).trim();
  if (!DECIMAL_RE.test(startStr) || !DECIMAL_RE.test(endStr)) {
    return { ok: false, error: 'invalid_range_format' };
  }

  const start = Number(startStr);
  const end = Number(endStr);
  if (end < start) {
    return { ok: false, error: 'range_reversed' };
  }
  if (end - start > MAX_RANGE_SPAN) {
    return { ok: false, error: 'range_too_large' };
  }

  return { ok: true, start, end };
}

export function parseKeyword(raw: string): KeywordResult {
  const keyword = raw.trim();
  if (keyword.length < 1 || keyword.length > MAX_KEYWORD_LEN) {
    return { ok: false, error: 'invalid_keyword' };
  }

  return { ok: true, keyword };
}
