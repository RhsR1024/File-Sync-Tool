import type { AlarmSubscriptionRecord } from './deviceSimulator';

export function indexSubscriptionsByDevice(records: readonly AlarmSubscriptionRecord[]) {
  const index = new Map<string, AlarmSubscriptionRecord[]>();
  for (const record of records) {
    const group = index.get(record.device_id);
    if (group) group.push(record);
    else index.set(record.device_id, [record]);
  }
  return index;
}

/** Preserve explicit expired selections and array identity when nothing changed. */
export function reconcileSubscriptionSelection(
  records: readonly AlarmSubscriptionRecord[],
  selected: string[],
  now: number,
): string[] {
  const available = new Set(records.map((record) => record.id));
  const next = selected.filter((id) => available.has(id));
  if (next.length === 0) {
    const active = records.filter((record) => record.expires_at_ms === null || record.expires_at_ms > now);
    if (active.length === 1) next.push(active[0].id);
  }
  return next.length === selected.length && next.every((id, index) => id === selected[index]) ? selected : next;
}
