import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'UmsInitialPasswordPage.vue'), 'utf8');

test('result table keeps each flow status on one line and lets long detail wrap', () => {
  assert.match(pageSource, /const umsResultStatusWrapClass = 'flex items-center gap-1\.5 whitespace-nowrap';/);
  assert.match(pageSource, /<table class="w-full table-fixed">/);
  assert.match(pageSource, /<td :class="umsResultMessageCellClass">\{\{ resultDetail\(result\) \}\}<\/td>/);
  assert.match(pageSource, /const umsResultMessageCellClass = 'px-6 py-3 text-sm text-slate-600 break-all';/);
});

test('all three flows are selected by default and carry their own factory old password', () => {
  assert.match(
    pageSource,
    /enabledFlows = ref<Record<UmsInitPasswordKind, boolean>>\(\{\s*framework: true,\s*ums: true,\s*cdm: true,\s*\}\)/,
  );
  assert.match(
    pageSource,
    /oldPasswords = ref<Record<UmsInitPasswordKind, string>>\(\{\s*framework: '123456',\s*ums: 'admin_123',\s*cdm: 'admin',\s*\}\)/,
  );
});

test('same-password conflict is evaluated per selected flow, not globally', () => {
  // UMS ships with admin_123, which is exactly what most people type as the new
  // password for the other two flows, so the check must name the offending flow.
  assert.match(
    pageSource,
    /conflictingFlows = computed\(\(\) =>\s*FLOWS\.filter\(flow => enabledFlows\.value\[flow\.kind\] && oldPasswords\.value\[flow\.kind\] === newPassword\.value\)/,
  );
  assert.match(pageSource, /samePasswordFor/);
  assert.ok(!pageSource.includes('samePasswordError'), 'the old global conflict message must be gone');
});

test('execution is blocked unless a flow, an IP and a new password are all present', () => {
  assert.match(pageSource, /allSelectedIps\.value\.length > 0 &&/);
  assert.match(pageSource, /selectedFlowCount\.value > 0 &&/);
  assert.match(pageSource, /newPassword\.value\.length > 0 &&/);
  assert.match(pageSource, /conflictingFlows\.value\.length === 0 &&/);
});

test('recent IPs migrate from the pre-rename storage key', () => {
  assert.match(pageSource, /const RECENT_IPS_KEY = 'umsInitialPassword\.recentIps';/);
  assert.match(pageSource, /const LEGACY_RECENT_IPS_KEY = 'frameworkPassword\.recentIps';/);
});
