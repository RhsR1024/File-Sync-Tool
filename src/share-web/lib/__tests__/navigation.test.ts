import { describe, expect, it } from 'vitest';

import { parentNodeIdFromBreadcrumbs } from '../navigation';
import type { FileShareBreadcrumb } from '../../types';

const crumbs = (...items: Array<[string | null, string]>): FileShareBreadcrumb[] =>
  items.map(([node_id, label]) => ({ node_id, label }));

describe('parentNodeIdFromBreadcrumbs', () => {
  it('returns undefined on the home page', () => {
    expect(parentNodeIdFromBreadcrumbs(crumbs([null, 'Home']))).toBeUndefined();
  });

  it('returns null for a share root so back navigates home', () => {
    expect(parentNodeIdFromBreadcrumbs(crumbs(
      [null, 'Home'],
      ['root.ums', 'UMS_TEMP'],
    ))).toBeNull();
  });

  it('returns the previous directory node for nested folders', () => {
    expect(parentNodeIdFromBreadcrumbs(crumbs(
      [null, 'Home'],
      ['root.ums', 'UMS_TEMP'],
      ['dir.parent', 'parent'],
      ['dir.child', 'child'],
    ))).toBe('dir.parent');
  });
});
