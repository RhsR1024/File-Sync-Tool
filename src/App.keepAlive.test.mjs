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
assert.doesNotMatch(
  appSource,
  /<keep-alive\s+include="[^"]*\bMainConsole\b/,
  'MainConsole is now kept alive by SyncConsolePage instead of the app shell',
);

console.log('App keep-alive tests PASSED');
