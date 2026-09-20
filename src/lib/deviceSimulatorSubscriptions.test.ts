import { describe, expect, it } from 'vitest';
import type { AlarmSubscriptionRecord } from './deviceSimulator';
import { indexSubscriptionsByDevice, reconcileSubscriptionSelection } from './deviceSimulatorSubscriptions';

function record(id: string, expires: number | null, device = 'device'): AlarmSubscriptionRecord {
  return { id, device_id: device, device_ip: '10.0.0.1', source_ip: '10.0.0.2', host: null,
    port: 80, duration_secs: null, learned_at_ms: 0, expires_at_ms: expires };
}

describe('device subscription selection', () => {
  it('indexes once without changing order or mixing devices', () => {
    const records = [record('a', null), record('b', null, 'other'), record('c', null)];
    const index = indexSubscriptionsByDevice(records);
    expect(index.get('device')).toEqual([records[0], records[2]]);
    expect(index.get('other')).toEqual([records[1]]);
  });
  it('preserves explicit expired selections and reuses unchanged arrays', () => {
    const selected = ['expired'];
    expect(reconcileSubscriptionSelection([record('expired', 10), record('active', null)], selected, 20)).toBe(selected);
  });
  it('selects the only active record, including after another expires', () => {
    const records = [record('first', 10), record('second', null)];
    const empty: string[] = [];
    expect(reconcileSubscriptionSelection(records, empty, 9)).toBe(empty);
    expect(reconcileSubscriptionSelection(records, empty, 10)).toEqual(['second']);
  });
  it('removes disappeared selections but preserves multiple valid selections in their original order', () => {
    const records = [record('first', null), record('second', null)];
    expect(reconcileSubscriptionSelection(records, ['second', 'gone', 'first'], 20)).toEqual(['second', 'first']);
    expect(reconcileSubscriptionSelection([], ['gone'], 20)).toEqual([]);
  });
});
