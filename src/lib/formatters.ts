/**
 * Locale-aware display formatters shared across pages.
 *
 * Pages that need to render dates / times consistently across the
 * Chinese and English locales should reach for these helpers instead
 * of calling `toLocaleString()` directly, which silently picks up the
 * host locale and ignores `useI18n().locale`.
 */

export type SupportedLocale = 'zh' | 'en' | string;

function resolveLocaleTag(locale: SupportedLocale | undefined): string {
  if (!locale) return 'en-US';
  if (locale.toLowerCase().startsWith('zh')) return 'zh-CN';
  return 'en-US';
}

/**
 * Format an ISO timestamp (or any value `Date` can parse) using the
 * BCP-47 tag derived from the active i18n locale.  Falls back to the
 * raw input when the value cannot be parsed.
 */
export function formatDateTime(
  value: string | number | Date | null | undefined,
  locale: SupportedLocale | undefined,
): string {
  if (value === null || value === undefined || value === '') return '';
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return typeof value === 'string' ? value : String(value);
  }
  return date.toLocaleString(resolveLocaleTag(locale));
}

/**
 * Format the `time` portion of a timestamp using the locale-aware tag.
 * Useful for dense log lines where only the wall-clock time matters.
 */
export function formatTime(
  value: string | number | Date | null | undefined,
  locale: SupportedLocale | undefined,
): string {
  if (value === null || value === undefined || value === '') return '';
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return typeof value === 'string' ? value : String(value);
  }
  return date.toLocaleTimeString(resolveLocaleTag(locale));
}
