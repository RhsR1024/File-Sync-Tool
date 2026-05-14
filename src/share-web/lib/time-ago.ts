export interface TimeAgoFormatter {
  justNow: string;
  minutes: (n: number) => string;
  hours: (n: number) => string;
  days: (n: number) => string;
  months: (n: number) => string;
  years: (n: number) => string;
}

function parseModified(value: string | null | undefined): number | null {
  if (!value) {
    return null;
  }
  const normalized = value.includes('T') ? value : value.replace(' ', 'T');
  const parsed = new Date(normalized);
  const ms = parsed.getTime();
  return Number.isNaN(ms) ? null : ms;
}

export function timeAgo(
  modified: string | null | undefined,
  format: TimeAgoFormatter,
  now: number = Date.now(),
): string {
  const ms = parseModified(modified);
  if (ms == null) {
    return '';
  }
  const diff = Math.max(0, (now - ms) / 1000);
  if (diff < 60) {
    return format.justNow;
  }
  if (diff < 3600) {
    return format.minutes(Math.floor(diff / 60));
  }
  if (diff < 86400) {
    return format.hours(Math.floor(diff / 3600));
  }
  if (diff < 86400 * 30) {
    return format.days(Math.floor(diff / 86400));
  }
  if (diff < 86400 * 365) {
    return format.months(Math.floor(diff / 86400 / 30));
  }
  return format.years(Math.floor(diff / 86400 / 365));
}
