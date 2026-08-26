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

test('whitelist details are collapsed by default and exposed by an accessible disclosure', () => {
  assert.match(pageSource, /showWhitelistDetail = ref<boolean>\(false\)/);
  assert.match(pageSource, /:aria-expanded="showWhitelistDetail"/);
  assert.match(pageSource, /v-show="showWhitelistDetail && addWhitelistRule"/);
});

test('HA access groups are placed in the right column above results', () => {
  const rightColumn = pageSource.indexOf('<!-- Right column: HA access groups + results -->');
  const haGroups = pageSource.indexOf("t('tools.applianceSsh.haGroupSection')", rightColumn);
  const results = pageSource.indexOf("t('tools.applianceSsh.results')", rightColumn);

  assert.ok(rightColumn >= 0);
  assert.ok(haGroups > rightColumn);
  assert.ok(results > haGroups);
});

test('recent IP chips toggle selection while the trash action only removes history', () => {
  assert.match(
    pageSource,
    /const toggleRecentIp = \(ip: string\) => \{[\s\S]*?selectedIps\.value = selectedIps\.value\.filter[\s\S]*?manualIpInputRef\.value\?\.removeTag\(ip\)[\s\S]*?manualIpInputRef\.value\?\.applyTag\(ip\)/,
  );
  assert.match(pageSource, /:aria-pressed="isRecentIpSelected\(ip\)"/);
  assert.match(pageSource, /@click="toggleRecentIp\(ip\)"/);
  assert.match(pageSource, /@click\.stop="removeRecentIp\(ip\)"/);
});
