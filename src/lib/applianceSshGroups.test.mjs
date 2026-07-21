import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  MAX_SLAVES_PER_GROUP,
  buildGroupTargets,
  buildRoleMap,
  composeAllTargets,
  createEmptyGroup,
  isGroupActive,
  isValidIp,
  normalizeGroup,
  parseGroupEntry,
  serializeGroup,
  swapGroupEndpoints,
  targetKey,
} from './applianceSshGroups.ts';

test('isValidIp accepts dotted quads and rejects malformed input', () => {
  assert.equal(isValidIp('192.168.1.10'), true);
  assert.equal(isValidIp('0.0.0.0'), true);
  assert.equal(isValidIp('255.255.255.255'), true);
  assert.equal(isValidIp('256.1.1.1'), false);
  assert.equal(isValidIp('1.2.3'), false);
  assert.equal(isValidIp('1.2.3.4.5'), false);
  assert.equal(isValidIp('a.b.c.d'), false);
  assert.equal(isValidIp(''), false);
});

test('normalizeGroup trims, dedupes slaves, and clips to the slave limit', () => {
  const g = normalizeGroup({
    master: ' 10.0.0.1 ',
    backup: ' 10.0.0.2 ',
    slaves: [' 10.0.0.3 ', '10.0.0.3', '', '10.0.0.4'],
  });
  assert.deepEqual(g, { master: '10.0.0.1', backup: '10.0.0.2', slaves: ['10.0.0.3', '10.0.0.4'] });

  const many = Array.from({ length: MAX_SLAVES_PER_GROUP + 3 }, (_, i) => `10.0.1.${i + 1}`);
  const clipped = normalizeGroup({ master: '10.0.0.1', backup: '', slaves: many });
  assert.equal(clipped.slaves.length, MAX_SLAVES_PER_GROUP);
  assert.equal(clipped.slaves[0], '10.0.1.1');
  assert.equal(clipped.slaves.at(-1), `10.0.1.${MAX_SLAVES_PER_GROUP}`);
});

test('isGroupActive requires a non-blank master', () => {
  assert.equal(isGroupActive(createEmptyGroup()), false);
  assert.equal(isGroupActive({ master: '  ', backup: '1.1.1.1', slaves: [] }), false);
  assert.equal(isGroupActive({ master: '1.1.1.1', backup: '', slaves: [] }), true);
});

test('swapGroupEndpoints exchanges master and backup without changing slaves', () => {
  const original = { master: '10.0.0.1', backup: '10.0.0.2', slaves: ['10.0.0.3'] };
  assert.deepEqual(swapGroupEndpoints(original), {
    master: '10.0.0.2',
    backup: '10.0.0.1',
    slaves: ['10.0.0.3'],
  });
  assert.deepEqual(original, {
    master: '10.0.0.1',
    backup: '10.0.0.2',
    slaves: ['10.0.0.3'],
  });
});

test('buildGroupTargets: master+backup+slaves produces a failover-capable jump pair', () => {
  const targets = buildGroupTargets({
    master: '10.0.0.1',
    backup: '10.0.0.2',
    slaves: ['10.0.0.3', '10.0.0.4'],
  });
  assert.deepEqual(targets, [
    { ip: '10.0.0.2', jumpHost: '10.0.0.1', allowFailover: true },
    { ip: '10.0.0.3' },
    { ip: '10.0.0.4' },
  ]);
});

test('buildGroupTargets: no backup degrades master to a direct target', () => {
  const targets = buildGroupTargets({ master: '10.0.0.1', backup: '', slaves: ['10.0.0.3'] });
  assert.deepEqual(targets, [{ ip: '10.0.0.1' }, { ip: '10.0.0.3' }]);
});

test('buildGroupTargets: master-only group and blank-master group', () => {
  assert.deepEqual(buildGroupTargets({ master: '10.0.0.1', backup: '', slaves: [] }), [
    { ip: '10.0.0.1' },
  ]);
  assert.deepEqual(buildGroupTargets({ master: '  ', backup: '10.0.0.2', slaves: ['10.0.0.3'] }), []);
});

test('composeAllTargets dedupes direct targets across manual input and groups', () => {
  const targets = composeAllTargets(
    ['10.0.0.5', ' 10.0.0.5 ', '10.0.0.1'],
    [
      { master: '10.0.0.1', backup: '', slaves: ['10.0.0.5'] },
      { master: '10.0.0.1', backup: '', slaves: [] },
    ],
  );
  assert.deepEqual(targets, [{ ip: '10.0.0.5' }, { ip: '10.0.0.1' }]);
});

test('composeAllTargets keeps a direct target and an HA pair containing the same ip', () => {
  const targets = composeAllTargets(
    ['10.0.0.2'],
    [{ master: '10.0.0.1', backup: '10.0.0.2', slaves: [] }],
  );
  assert.deepEqual(targets, [
    { ip: '10.0.0.2' },
    { ip: '10.0.0.2', jumpHost: '10.0.0.1', allowFailover: true },
  ]);
});

test('composeAllTargets dedupes identical jump pairs across groups', () => {
  const targets = composeAllTargets(
    [],
    [
      { master: '10.0.0.1', backup: '10.0.0.2', slaves: [] },
      { master: '10.0.0.1', backup: '10.0.0.2', slaves: ['10.0.0.9'] },
    ],
  );
  assert.deepEqual(targets, [
    { ip: '10.0.0.2', jumpHost: '10.0.0.1', allowFailover: true },
    { ip: '10.0.0.9' },
  ]);
});

test('buildRoleMap assigns masterBackup to both pair directions and direct roles elsewhere', () => {
  const map = buildRoleMap([
    { master: '10.0.0.1', backup: '10.0.0.2', slaves: ['10.0.0.3'] },
    { master: '10.0.1.1', backup: '', slaves: ['10.0.1.2'] },
  ]);
  assert.deepEqual(map.get(targetKey({ ip: '10.0.0.2', jumpHost: '10.0.0.1' })), {
    groupIndex: 0,
    role: 'masterBackup',
  });
  assert.deepEqual(map.get(targetKey({ ip: '10.0.0.1', jumpHost: '10.0.0.2' })), {
    groupIndex: 0,
    role: 'masterBackup',
  });
  assert.deepEqual(map.get(targetKey({ ip: '10.0.0.3' })), { groupIndex: 0, role: 'slave' });
  assert.deepEqual(map.get(targetKey({ ip: '10.0.1.1' })), { groupIndex: 1, role: 'master' });
  assert.deepEqual(map.get(targetKey({ ip: '10.0.1.2' })), { groupIndex: 1, role: 'slave' });
  assert.equal(map.get(targetKey({ ip: '10.0.0.1' })), undefined);
});

test('buildRoleMap keeps the first writer on duplicate keys', () => {
  const map = buildRoleMap([
    { master: '10.0.0.1', backup: '', slaves: [] },
    { master: '10.0.0.1', backup: '', slaves: [] },
  ]);
  assert.deepEqual(map.get(targetKey({ ip: '10.0.0.1' })), { groupIndex: 0, role: 'master' });
});

test('serializeGroup and parseGroupEntry round-trip the three-segment format', () => {
  const group = { master: '10.0.0.1', backup: '10.0.0.2', slaves: ['10.0.0.3', '10.0.0.4'] };
  const raw = serializeGroup(group);
  assert.equal(raw, '10.0.0.1=>10.0.0.2=>10.0.0.3,10.0.0.4');
  assert.deepEqual(parseGroupEntry(raw), group);

  const noBackup = { master: '10.0.0.1', backup: '', slaves: ['10.0.0.3'] };
  assert.deepEqual(parseGroupEntry(serializeGroup(noBackup)), noBackup);

  const masterOnly = { master: '10.0.0.1', backup: '', slaves: [] };
  assert.deepEqual(parseGroupEntry(serializeGroup(masterOnly)), masterOnly);
});

test('parseGroupEntry accepts the legacy two-segment jump-pair format', () => {
  assert.deepEqual(parseGroupEntry('10.0.0.1=>10.0.0.2'), {
    master: '10.0.0.1',
    backup: '10.0.0.2',
    slaves: [],
  });
  assert.deepEqual(parseGroupEntry(' 10.0.0.1 => 10.0.0.2 '), {
    master: '10.0.0.1',
    backup: '10.0.0.2',
    slaves: [],
  });
});

test('parseGroupEntry rejects malformed entries', () => {
  assert.equal(parseGroupEntry(''), null);
  assert.equal(parseGroupEntry('10.0.0.1'), null);
  assert.equal(parseGroupEntry('10.0.0.1=>'), null);
  assert.equal(parseGroupEntry('=>10.0.0.2'), null);
  assert.equal(parseGroupEntry('a=>b=>c=>d'), null);
  assert.equal(parseGroupEntry('=>=>'), null);
});

test('parseGroupEntry clips slaves beyond the limit', () => {
  const many = Array.from({ length: MAX_SLAVES_PER_GROUP + 2 }, (_, i) => `10.0.1.${i + 1}`);
  const parsed = parseGroupEntry(`10.0.0.1=>=>${many.join(',')}`);
  assert.equal(parsed.slaves.length, MAX_SLAVES_PER_GROUP);
});
