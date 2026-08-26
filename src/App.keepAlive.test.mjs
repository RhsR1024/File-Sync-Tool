import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('./App.vue', import.meta.url), 'utf8');

assert.match(
  appSource,
  /<keep-alive\s+include="[^"]*\bRemotePackagePatchPage\b/,
  'RemotePackagePatchPage must stay alive so entered SSH data survives route tab switches',
);

assert.match(
  appSource,
  /<keep-alive\s+include="[^"]*\bSyncConsolePage\b/,
  'SyncConsolePage must stay alive so nested tab state survives main-route switches',
);
for (const pageName of ['ToolsHubPage', 'AboutPage', 'ErrorCodeLookupPage']) {
  assert.match(
    appSource,
    new RegExp(`<keep-alive\\s+include="[^"]*\\b${pageName}\\b`),
    `${pageName} is resource-free and should stay alive across navigation`,
  );
}
assert.doesNotMatch(
  appSource,
  /<keep-alive\s+include="[^"]*\bMainConsole\b/,
  'MainConsole is now kept alive by SyncConsolePage instead of the app shell',
);

assert.match(appSource, /<AppTitleBar \/>/, 'the main layout must render the custom window title bar');
assert.match(appSource, /'app-window-shell--maximized': isMaximized/, 'maximized windows must disable custom rounded chrome');
assert.doesNotMatch(appSource, /shadow-\[0_24px_60px/, 'the app shell must not draw a clipped outer shadow');
assert.match(appSource, /mainWindow\.onResized/, 'window chrome must track maximize and restore changes');
assert.match(appSource, /:aria-busy="isNavigating"/, 'navigation must expose a non-layout-shifting busy state');
assert.match(appSource, /v-show="isNavigating"/, 'slow route loads must show immediate visual feedback');

console.log('App keep-alive tests PASSED');
