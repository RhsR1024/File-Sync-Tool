import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('./App.vue', import.meta.url), 'utf8');

assert.match(
  appSource,
  /<keep-alive\s+include="[^"]*\bRemotePackagePatchPage\b/,
  'RemotePackagePatchPage must stay alive so entered SSH data survives route tab switches',
);

console.log('App keep-alive tests PASSED');
