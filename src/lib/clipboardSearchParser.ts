export interface ParsedClipboardSearch {
  keywords: string[];
  filters: {
    type?: 'text' | 'html' | 'image' | 'file';
    from?: string;        // YYYY-MM-DD
    to?: string;          // YYYY-MM-DD
    app?: string;
    fav?: boolean;
    sizeGt?: number;
    sizeLt?: number;
  };
}

const VALID_KINDS = new Set(['text', 'html', 'image', 'file']);

export function parseSearch(input: string): ParsedClipboardSearch {
  const result: ParsedClipboardSearch = { keywords: [], filters: {} };
  const tokens = input.trim().split(/\s+/).filter(Boolean);

  for (const tok of tokens) {
    const colonIdx = tok.indexOf(':');
    if (colonIdx <= 0) {
      result.keywords.push(tok);
      continue;
    }
    const key = tok.slice(0, colonIdx).toLowerCase();
    const val = tok.slice(colonIdx + 1);

    switch (key) {
      case 'type':
        if (VALID_KINDS.has(val.toLowerCase())) {
          result.filters.type = val.toLowerCase() as ParsedClipboardSearch['filters']['type'];
        } else {
          result.keywords.push(tok);
        }
        break;
      case 'from':
        if (val) result.filters.from = val;
        break;
      case 'to':
        if (val) result.filters.to = val;
        break;
      case 'app':
        if (val) result.filters.app = val;
        break;
      case 'fav':
        result.filters.fav = true;
        break;
      case 'size':
        if (val.startsWith('>')) {
          const n = Number.parseInt(val.slice(1), 10);
          if (Number.isFinite(n)) result.filters.sizeGt = n;
        } else if (val.startsWith('<')) {
          const n = Number.parseInt(val.slice(1), 10);
          if (Number.isFinite(n)) result.filters.sizeLt = n;
        } else {
          result.keywords.push(tok);
        }
        break;
      default:
        result.keywords.push(tok);
    }
  }

  return result;
}
