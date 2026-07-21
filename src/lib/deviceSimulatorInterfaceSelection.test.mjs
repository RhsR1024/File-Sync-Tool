import assert from 'node:assert/strict';
import { test } from 'node:test';

import { recommendSimulatorInterface } from './deviceSimulatorInterfaceSelection.ts';

function adapter(id, addresses) {
  return {
    id,
    name: id,
    description: id,
    is_enabled: true,
    is_up: true,
    ipv4_addresses: addresses,
  };
}

test('selects the only adapter whose real subnet contains the virtual device IP', () => {
  const result = recommendSimulatorInterface([
    adapter('office', ['10.20.30.5/24']),
    adapter('device-lan', ['192.168.50.2/24']),
  ], '192.168.50.100', [], 'office');

  assert.equal(result.recommended_interface_id, 'device-lan');
  assert.equal(result.kind, 'matched');
  assert.equal(result.matched_network, '192.168.50.0/24');
});

test('preserves the saved adapter when multiple adapters match equally', () => {
  const result = recommendSimulatorInterface([
    adapter('ethernet-1', ['192.168.50.2/24']),
    adapter('ethernet-2', ['192.168.50.3/24']),
  ], '192.168.50.100', [], 'ethernet-2');

  assert.equal(result.recommended_interface_id, 'ethernet-2');
  assert.equal(result.kind, 'ambiguous');
  assert.deepEqual(result.matching_interface_ids, ['ethernet-1', 'ethernet-2']);
});

test('uses the adapter matching the most explicit device addresses', () => {
  const result = recommendSimulatorInterface([
    adapter('first-lan', ['192.168.10.2/24']),
    adapter('second-lan', ['192.168.20.2/24']),
  ], '192.168.10.100', ['192.168.20.10', '192.168.20.11', '192.168.10.10'], 'first-lan');

  assert.equal(result.recommended_interface_id, 'second-lan');
  assert.equal(result.matched_target_count, 2);
  assert.equal(result.target_count, 3);
});

test('keeps a valid saved adapter as an explicit fallback when no subnet matches', () => {
  const result = recommendSimulatorInterface([
    adapter('saved', ['10.0.0.2/24']),
    adapter('other', ['172.16.0.2/24']),
  ], '192.168.50.100', [], 'saved');

  assert.equal(result.recommended_interface_id, 'saved');
  assert.equal(result.kind, 'fallback');
  assert.equal(result.matched_target_count, 0);
});
