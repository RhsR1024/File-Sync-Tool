import assert from 'node:assert/strict';

import { SIDEBAR_NAV_SECTIONS, isSidebarItemActive } from './sidebarNavigation.ts';

assert.equal(SIDEBAR_NAV_SECTIONS.length, 3);
assert.deepEqual(
  SIDEBAR_NAV_SECTIONS.map((section) => section.labelKey),
  ['sidebar.commonGroup', 'sidebar.tools', 'sidebar.systemGroup'],
);

assert.deepEqual(
  SIDEBAR_NAV_SECTIONS[0].items.map((item) => item.path),
  ['/sync', '/'],
  'common navigation should expose only the sync console and global logs',
);
assert.equal(
  isSidebarItemActive('/sync/delivery', SIDEBAR_NAV_SECTIONS[0].items[0]),
  true,
  'the sync-console entry should stay active for every nested tab',
);
assert.equal(
  isSidebarItemActive('/', SIDEBAR_NAV_SECTIONS[0].items[1]),
  true,
  'global logs should be active only at the app root',
);
assert.equal(
  isSidebarItemActive('/sync', SIDEBAR_NAV_SECTIONS[0].items[1]),
  false,
  'global logs should not stay active inside sync console routes',
);

const toolPaths = SIDEBAR_NAV_SECTIONS[1].items.map((item) => item.path);
assert.deepEqual(toolPaths, [
  '/tools',
  '/tools/appliance-ssh',
  '/tools/remote-package-patch',
  '/tools/framework-password',
  '/tools/code-statistics',
  '/tools/network',
  '/tools/display-control',
  '/tools/screen-share',
  '/tools/video-device-simulator',
  '/tools/file-share',
  '/tools/disk-cache-cleanup',
  '/tools/clipboard',
  '/tools/error-code-lookup',
  '/tools/notepad-extensions',
]);

assert.equal(
  isSidebarItemActive('/tools', SIDEBAR_NAV_SECTIONS[1].items[0]),
  true,
  'tools overview should match the overview route exactly',
);
assert.equal(
  isSidebarItemActive(
    '/tools/video-device-simulator/session',
    SIDEBAR_NAV_SECTIONS[1].items.find((item) => item.key === 'video-device-simulator'),
  ),
  true,
  'the simulator entry should remain active for nested simulator routes',
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
