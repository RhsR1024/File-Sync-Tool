import type { ApplianceSshTarget } from './tauri';

/**
 * HA access group for the appliance SSH page: one master, an optional backup,
 * and up to MAX_SLAVES_PER_GROUP standalone slaves. A master/backup pair can
 * switch roles, so either node may provide the management API and act as the
 * SSH hop for the other node.
 */
export interface HaAccessGroup {
  master: string;
  backup: string;
  slaves: string[];
}

export type HaRole = 'masterBackup' | 'master' | 'slave';

export interface HaRoleInfo {
  groupIndex: number;
  role: HaRole;
}

export const MAX_SLAVES_PER_GROUP = 10;

const GROUP_SEP = '=>';
const SLAVE_SEP = ',';

export function isValidIp(ip: string): boolean {
  const parts = ip.split('.');
  if (parts.length !== 4) return false;
  return parts.every(p => /^\d+$/.test(p) && Number(p) >= 0 && Number(p) <= 255);
}

export function createEmptyGroup(): HaAccessGroup {
  return { master: '', backup: '', slaves: [] };
}

export function swapGroupEndpoints(group: HaAccessGroup): HaAccessGroup {
  return {
    master: group.backup,
    backup: group.master,
    slaves: [...group.slaves],
  };
}

export function normalizeGroup(group: HaAccessGroup): HaAccessGroup {
  const seen = new Set<string>();
  const slaves: string[] = [];
  for (const raw of group.slaves) {
    const ip = raw.trim();
    if (!ip || seen.has(ip)) continue;
    seen.add(ip);
    slaves.push(ip);
  }
  return {
    master: group.master.trim(),
    backup: group.backup.trim(),
    slaves: slaves.slice(0, MAX_SLAVES_PER_GROUP),
  };
}

export function isGroupActive(group: HaAccessGroup): boolean {
  return group.master.trim() !== '';
}

/**
 * Expand one group into backend targets. For a master/backup pair, use the
 * master management API and chain SSH from master to backup. The backend may
 * reverse the direction after a failover. Without a backup, the master remains
 * a direct target.
 */
export function buildGroupTargets(group: HaAccessGroup): ApplianceSshTarget[] {
  const g = normalizeGroup(group);
  if (!g.master) return [];

  const targets: ApplianceSshTarget[] = [];
  if (g.backup) {
    targets.push({ ip: g.backup, jumpHost: g.master, allowFailover: true });
  } else {
    targets.push({ ip: g.master });
  }
  for (const slave of g.slaves) {
    targets.push({ ip: slave });
  }
  return targets;
}

export function targetKey(target: ApplianceSshTarget): string {
  return `${target.jumpHost ?? ''}${GROUP_SEP}${target.ip}`;
}

/**
 * Merge direct IPs (manual input + server checkboxes) with all group targets,
 * de-duplicated by (jumpHost, ip) so a direct target and a behind-jump target
 * with the same ip stay distinct.
 */
export function composeAllTargets(
  directIps: readonly string[],
  groups: readonly HaAccessGroup[],
): ApplianceSshTarget[] {
  const targets: ApplianceSshTarget[] = [];
  const seen = new Set<string>();
  const push = (target: ApplianceSshTarget) => {
    const key = targetKey(target);
    if (seen.has(key)) return;
    seen.add(key);
    targets.push(target);
  };

  for (const raw of directIps) {
    const ip = raw.trim();
    if (ip) push({ ip });
  }
  for (const group of groups) {
    for (const target of buildGroupTargets(group)) {
      push(target);
    }
  }
  return targets;
}

/**
 * Map each group-originated target key to its group index and role so result
 * rows can show a role badge. Both directions of a failover-capable pair map
 * to the same combined role, because the backend reports the direction that
 * actually succeeded. First writer wins on duplicates.
 */
export function buildRoleMap(groups: readonly HaAccessGroup[]): Map<string, HaRoleInfo> {
  const map = new Map<string, HaRoleInfo>();
  const set = (key: string, info: HaRoleInfo) => {
    if (!map.has(key)) map.set(key, info);
  };

  groups.forEach((raw, groupIndex) => {
    const g = normalizeGroup(raw);
    if (!g.master) return;
    if (g.backup) {
      set(targetKey({ ip: g.master, jumpHost: g.backup }), { groupIndex, role: 'masterBackup' });
      set(targetKey({ ip: g.backup, jumpHost: g.master }), { groupIndex, role: 'masterBackup' });
    } else {
      set(targetKey({ ip: g.master }), { groupIndex, role: 'master' });
    }
    for (const slave of g.slaves) {
      set(targetKey({ ip: slave }), { groupIndex, role: 'slave' });
    }
  });
  return map;
}

/** Serialize as `master=>backup=>slave1,slave2` for the recent-history kv list. */
export function serializeGroup(group: HaAccessGroup): string {
  const g = normalizeGroup(group);
  return [g.master, g.backup, g.slaves.join(SLAVE_SEP)].join(GROUP_SEP);
}

/**
 * Parse a recent-history entry. Accepts the legacy two-segment jump-pair
 * format `master=>backup` (both required) and the current three-segment
 * format where backup and slaves may be empty.
 */
export function parseGroupEntry(raw: string): HaAccessGroup | null {
  if (typeof raw !== 'string') return null;
  const segments = raw.split(GROUP_SEP);
  if (segments.length < 2 || segments.length > 3) return null;

  const master = (segments[0] ?? '').trim();
  const backup = (segments[1] ?? '').trim();
  const slaves = (segments[2] ?? '')
    .split(SLAVE_SEP)
    .map(s => s.trim())
    .filter(Boolean);

  const group = normalizeGroup({ master, backup, slaves });
  if (!group.master) return null;
  if (segments.length === 2 && !group.backup) return null;
  return group;
}
