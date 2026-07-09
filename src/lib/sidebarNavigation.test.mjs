import assert from 'node:assert/strict';

import { SIDEBAR_NAV_SECTIONS, isSidebarItemActive } from './sidebarNavigation.ts';

assert.equal(SIDEBAR_NAV_SECTIONS.length, 3);
assert.deepEqual(
  SIDEBAR_NAV_SECTIONS.map((section) => section.labelKey),
  ['sidebar.commonGroup', 'sidebar.tools', 'sidebar.systemGroup'],
);

const toolPaths = SIDEBAR_NAV_SECTIONS[1].items.map((item) => item.path);
assert.deepEqual(toolPaths, [
  '/tools',
  '/tools/appliance-ssh',
  '/tools/remote-package-patch',
  '/tools/framework-password',
  '/tools/code-statistics',
  '/tools/network',
  '/tools/screen-share',
  '/tools/file-share',
  '/tools/disk-cache-cleanup',
  '/tools/clipboard',
  '/tools/error-code-lookup',
]);

assert.equal(
  isSidebarItemActive('/tools', SIDEBAR_NAV_SECTIONS[1].items[0]),
  true,
  'tools overview should match the overview route exactly',
);
assert.equal(
  isSidebarItemActive('/tools/appliance-ssh', SIDEBAR_NAV_SECTIONS[1].items[0]),
  false,
  'tools overview should not stay active on child tool routes',
);
assert.equal(
  isSidebarItemActive('/tools/appliance-ssh/details', SIDEBAR_NAV_SECTIONS[1].items[1]),
  true,
  'tool items should stay active for nested tool sub-routes',
);

console.log('sidebarNavigation tests PASSED');
