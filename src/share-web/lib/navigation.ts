import type { FileShareBreadcrumb } from '../types';

export function parentNodeIdFromBreadcrumbs(
  breadcrumbs: FileShareBreadcrumb[],
): string | null | undefined {
  if (breadcrumbs.length <= 1) {
    return undefined;
  }
  return breadcrumbs[breadcrumbs.length - 2]?.node_id ?? null;
}
