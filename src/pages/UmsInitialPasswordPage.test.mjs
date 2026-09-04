import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'UmsInitialPasswordPage.vue'), 'utf8');
const messagesSource = readFileSync(join(__dirname, '..', 'locales', 'messages.ts'), 'utf8');
const formStoreSource = readFileSync(
  join(__dirname, '..', 'lib', 'umsInitialPasswordForm.ts'),
  'utf8',
);

test('Chinese sidebar and page title use the spaced UMS branding', () => {
  assert.ok(messagesSource.includes("umsInitialPassword: 'UMS 初始密码修改'"));
  assert.ok(messagesSource.includes("title: 'UMS 初始密码修改'"));
  assert.ok(!messagesSource.includes('UMS初始密码修改'));
});

test('result table keeps each flow status on one line and lets long detail wrap', () => {
  assert.match(pageSource, /const umsResultStatusWrapClass = 'flex items-center gap-1\.5 whitespace-nowrap';/);
  assert.match(pageSource, /<table class="w-full table-fixed">/);
  assert.match(pageSource, /<td :class="umsResultMessageCellClass">\{\{ resultDetail\(result\) \}\}<\/td>/);
  assert.match(pageSource, /const umsResultMessageCellClass = 'px-6 py-3 text-sm text-slate-600 break-all';/);
});

test('all three flows are selected by default and carry their own factory old password', () => {
  assert.match(
    formStoreSource,
    /enabledFlows: \{ framework: true, ums: true, cdm: true \}/,
  );
  assert.match(
    formStoreSource,
    /DEFAULT_OLD_PASSWORDS: Record<UmsInitPasswordKind, string> = \{\s*framework: '123456',\s*ums: 'admin_123',\s*cdm: 'admin',\s*\}/,
  );
});

test('same-password conflict is evaluated per selected flow, not globally', () => {
  assert.match(
    pageSource,
    /const isSameAsNew = \(kind: UmsInitPasswordKind\) =>\s*form\.enabledFlows\[kind\] && form\.oldPasswords\[kind\] === form\.newPassword;/,
  );
  assert.match(pageSource, /samePasswordFor/);
  assert.ok(!pageSource.includes('samePasswordError'), 'the old global conflict message must be gone');
});

test('identical UMS passwords set the init flag instead of blocking', () => {
  // UMS ships with admin_123, the very value most people type as the new password.
  // That case means "already at target" and must stay runnable; framework and CDM
  // have no pwdIsInit equivalent, so for them it remains a hard conflict.
  assert.match(
    pageSource,
    /conflictingFlows = computed\(\(\) =>\s*FLOWS\.filter\(flow => flow\.kind !== 'ums' && isSameAsNew\(flow\.kind\)\),?\s*\);/,
  );
  assert.match(
    pageSource,
    /initFlagOnlyFlows = computed\(\(\) => FLOWS\.filter\(flow => flow\.kind === 'ums' && isSameAsNew\(flow\.kind\)\)\);/,
  );
  assert.match(pageSource, /initFlagOnlyHint/);
});

test('execution is blocked unless a flow, an IP and a new password are all present', () => {
  assert.match(pageSource, /allSelectedIps\.value\.length > 0 &&/);
  assert.match(pageSource, /selectedFlowCount\.value > 0 &&/);
  assert.match(pageSource, /form\.newPassword\.length > 0 &&/);
  assert.match(pageSource, /conflictingFlows\.value\.length === 0 &&/);
});

test('form fields survive a tab switch by living in a module-scoped store', () => {
  // Every user-editable field must read/write the shared store, never a local
  // ref, otherwise unmounting the page on tab switch drops what was typed.
  assert.match(pageSource, /umsInitialPasswordFormState as form/);
  for (const binding of [
    'v-model="form.manualIpInput"',
    'v-model="form.newPassword"',
    'v-model="form.oldPasswords[flow.kind]"',
    ':checked="form.enabledFlows[flow.kind]"',
  ]) {
    assert.ok(pageSource.includes(binding), `template must bind ${binding}`);
  }
  assert.ok(
    !/const (newPassword|oldPasswords|enabledFlows|manualIpInput) = ref/.test(pageSource),
    'form fields must not be re-declared as local refs',
  );
});

test('passwords are kept out of localStorage', () => {
  // PersistedShape has no nested braces, so a non-greedy body match is exact.
  // Match field names only — the value types mention UmsInitPasswordKind, which
  // would trip a naive "contains Password" check.
  const persisted = formStoreSource.match(/interface PersistedShape \{([^}]*)\}/)[1];
  const fields = persisted
    .split('\n')
    .map(line => line.trim().split(':')[0])
    .filter(Boolean);
  assert.deepEqual(fields, ['selectedIps', 'manualIpTags', 'manualIpInput', 'enabledFlows']);
  assert.ok(
    !fields.some(field => /password/i.test(field)),
    'PersistedShape must not carry password fields',
  );
});

test('the 1 second API timeout option is gone and out-of-range values fall back to 5', () => {
  // 1s was too tight for the UMS public-key and password-change calls.
  assert.match(pageSource, /const API_TIMEOUT_OPTIONS = \[3, 5, 10, 30\] as const;/);
  assert.match(pageSource, /const DEFAULT_API_TIMEOUT_SECS = 5;/);
  assert.match(pageSource, /normalizeApiTimeout/);
  assert.ok(!pageSource.includes(':value="1"'), 'the 1 second option must be removed');
});

test('recent IPs migrate from the pre-rename storage key', () => {
  assert.match(pageSource, /const RECENT_IPS_KEY = 'umsInitialPassword\.recentIps';/);
  assert.match(pageSource, /const LEGACY_RECENT_IPS_KEY = 'frameworkPassword\.recentIps';/);
});

test('recent IP chips toggle selection while the trash action only removes history', () => {
  assert.match(
    pageSource,
    /const toggleRecentIp = \(ip: string\) => \{[\s\S]*?form\.selectedIps = form\.selectedIps\.filter[\s\S]*?removeManualIpTag\(ip\)[\s\S]*?addManualIpTag\(ip\)/,
  );
  assert.match(pageSource, /:aria-pressed="isRecentIpSelected\(ip\)"/);
  assert.match(pageSource, /@click="toggleRecentIp\(ip\)"/);
  assert.match(pageSource, /@click\.stop="removeRecentIp\(ip\)"/);
});
