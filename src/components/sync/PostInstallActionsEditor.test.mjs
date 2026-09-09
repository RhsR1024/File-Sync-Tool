import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const componentSource = readFileSync(join(__dirname, 'PostInstallActionsEditor.vue'), 'utf8');

test('post-install password fields are always shown in plain text', () => {
  assert.equal((componentSource.match(/type="password"/g) ?? []).length, 0);
  assert.equal((componentSource.match(/type="text"/g) ?? []).length, 3);
  for (const binding of [
    'v-model="model.passwords[`${kind}_old_password`]" type="text"',
    'v-model="model.passwords[`${kind}_new_password`]" type="text"',
    'v-model="model.topology.framework_password" type="text"',
  ]) {
    assert.ok(componentSource.includes(binding), `password field must use ${binding}`);
  }
  assert.ok(!componentSource.includes('showPassword'));
  assert.ok(!componentSource.includes('hidePassword'));
});
