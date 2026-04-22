interface DiskCleanupServerLike {
  enabled: boolean;
  host: string;
}

export function getSuggestedDiskCleanupHosts(
  servers: readonly DiskCleanupServerLike[] | null | undefined,
  recentHosts: readonly string[] | null | undefined,
): string[] {
  const recentHostSet = new Set(
    Array.isArray(recentHosts)
      ? recentHosts.map((item) => item.trim()).filter(Boolean)
      : [],
  );
  const uniqueHosts = new Set<string>();

  for (const server of servers ?? []) {
    if (!server?.enabled) {
      continue;
    }

    const host = server.host?.trim();
    if (!host || recentHostSet.has(host) || uniqueHosts.has(host)) {
      continue;
    }

    uniqueHosts.add(host);
  }

  return Array.from(uniqueHosts);
}
