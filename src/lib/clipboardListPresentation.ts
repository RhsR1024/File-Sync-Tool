import { parseSearch } from './clipboardSearchParser.ts';
import type {
  ClipboardSourceAppDisplay,
  ClipboardTimeFormat,
} from './clipboardTypes.ts';

export interface ClipboardHighlightPart {
  text: string;
  match: boolean;
}

export interface ClipboardRelativeTimeLabels {
  justNow: string;
  today: string;
  yesterday: string;
  minutesAgo: (minutes: number) => string;
}

export interface ClipboardSourceAppPresentation {
  showIcon: boolean;
  showName: boolean;
}

export function extractClipboardSearchKeywords(search: string): string[] {
  return parseSearch(search)
    .keywords
    .map((keyword) => keyword.trim())
    .filter(Boolean);
}

export function buildClipboardHighlightParts(
  text: string,
  keywords: string[],
): ClipboardHighlightPart[] {
  if (!text) return [];

  const normalizedKeywords = Array.from(
    new Set(
      keywords
        .map((keyword) => keyword.trim())
        .filter(Boolean)
        .sort((left, right) => right.length - left.length),
    ),
  );
  if (normalizedKeywords.length === 0) {
    return [{ text, match: false }];
  }

  const matcher = new RegExp(
    `(${normalizedKeywords.map(escapeRegExp).join('|')})`,
    'gi',
  );
  const parts: ClipboardHighlightPart[] = [];
  let lastIndex = 0;

  for (const match of text.matchAll(matcher)) {
    const start = match.index ?? 0;
    if (start > lastIndex) {
      parts.push({
        text: text.slice(lastIndex, start),
        match: false,
      });
    }

    const matchedText = match[0];
    if (!matchedText) continue;

    parts.push({
      text: matchedText,
      match: true,
    });
    lastIndex = start + matchedText.length;
  }

  if (lastIndex < text.length) {
    parts.push({
      text: text.slice(lastIndex),
      match: false,
    });
  }

  return parts.length > 0 ? parts : [{ text, match: false }];
}

export function resolveSourceAppPresentation(
  mode: ClipboardSourceAppDisplay,
  sourceApp: string | null,
  sourceAppIcon: string | null,
): ClipboardSourceAppPresentation {
  const hasName = Boolean(sourceApp?.trim());
  const hasIcon = Boolean(sourceAppIcon?.trim());
  const hasAnySource = hasName || hasIcon;

  if (!hasAnySource) {
    return {
      showIcon: false,
      showName: false,
    };
  }

  switch (mode) {
    case 'icon':
      return {
        showIcon: true,
        showName: false,
      };
    case 'both':
      return {
        showIcon: true,
        showName: hasName,
      };
    case 'none':
      return {
        showIcon: false,
        showName: false,
      };
    case 'name':
    default:
      return {
        showIcon: false,
        showName: hasName,
      };
  }
}

export function formatClipboardTimeLabel(
  timestampMs: number,
  timeFormat: ClipboardTimeFormat,
  labels: ClipboardRelativeTimeLabels,
  now = new Date(),
): string {
  const date = new Date(timestampMs);
  if (timeFormat === 'absolute') {
    const year = String(date.getFullYear());
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    const hours = String(date.getHours()).padStart(2, '0');
    const minutes = String(date.getMinutes()).padStart(2, '0');
    return `${year}-${month}-${day} ${hours}:${minutes}`;
  }

  const diffSec = Math.floor((now.getTime() - timestampMs) / 1000);
  if (diffSec < 60) return labels.justNow;
  if (diffSec < 3600) {
    return labels.minutesAgo(Math.floor(diffSec / 60));
  }

  const sameDay =
    date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate();
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  if (sameDay) return `${labels.today} ${hours}:${minutes}`;

  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const isYesterday =
    date.getFullYear() === yesterday.getFullYear()
    && date.getMonth() === yesterday.getMonth()
    && date.getDate() === yesterday.getDate();
  if (isYesterday) return `${labels.yesterday} ${hours}:${minutes}`;

  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${month}-${day} ${hours}:${minutes}`;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
