import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'EnableApplianceSshPage.vue'), 'utf8');

test('allow all is the first whitelist source and remains the default', () => {
  assert.match(pageSource, /whitelistSourceMode = ref<'local' \| 'all'>\('all'\)/);
  const allOption = pageSource.indexOf("t('tools.applianceSsh.whitelistSourceAll')");
  const localOption = pageSource.indexOf("t('tools.applianceSsh.whitelistSourceLocal')");
  assert.ok(allOption >= 0);
  assert.ok(localOption >= 0);
  assert.ok(allOption < localOption);
});

test('whitelist source modes keep their existing request mapping', () => {
  assert.match(pageSource, /const WHITELIST_ALL_CIDR = '0\.0\.0\.0\/0'/);
  assert.match(
    pageSource,
    /whitelistSourceMode\.value === 'all'[\s\S]*?WHITELIST_ALL_CIDR[\s\S]*?: undefined/,
  );
});
