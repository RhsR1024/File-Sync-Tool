function cleanRecentItems(values: readonly string[]): string[] {
  const seen = new Set<string>();
  const items: string[] = [];

  for (const rawValue of values) {
    const value = rawValue.trim();
    if (!value || seen.has(value)) {
      continue;
    }
    seen.add(value);
    items.push(value);
  }

  return items;
}

export function normalizeRecentItems(
  values: readonly string[] | null | undefined,
  limit = 10,
): string[] {
  if (!Array.isArray(values) || limit <= 0) {
    return [];
  }

  return cleanRecentItems(values).slice(0, limit);
}

export function mergeRecentItems(
  existing: readonly string[] | null | undefined,
  incoming: readonly string[] | string,
  limit = 10,
): string[] {
  const incomingValues = Array.isArray(incoming) ? incoming : [incoming];
  return normalizeRecentItems(
    [
      ...incomingValues,
      ...(Array.isArray(existing) ? existing : []),
    ],
    limit,
  );
}
