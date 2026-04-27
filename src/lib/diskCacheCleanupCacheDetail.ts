const PREVIEW_FIELDS = ['storageId', 'traceId'] as const;

type PreviewField = typeof PREVIEW_FIELDS[number];

function isPreviewRecord(value: unknown): value is Record<PreviewField, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function formatPreviewLine(field: PreviewField, value: unknown): string | null {
  if (typeof value !== 'string' || value.length === 0) {
    return null;
  }
  return `"${field}":${JSON.stringify(value)}`;
}

export function buildCacheDetailPreview(fullValue: string, fallbackPreview: string): string {
  try {
    const parsed = JSON.parse(fullValue);
    if (isPreviewRecord(parsed)) {
      const lines = PREVIEW_FIELDS
        .map((field) => formatPreviewLine(field, parsed[field]))
        .filter((line): line is string => line !== null);

      if (lines.length > 0) {
        return lines.join('\n');
      }
    }
  } catch {
    // Not every Redis value is JSON; fall back to the backend preview text.
  }

  return fallbackPreview;
}
