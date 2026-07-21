import type { SimulatorNetworkInterfaceInfo } from './deviceSimulator';

export type SimulatorInterfaceSelectionKind =
  | 'matched'
  | 'ambiguous'
  | 'fallback'
  | 'invalid_target'
  | 'unavailable';

export interface SimulatorInterfaceSelection {
  recommended_interface_id: string;
  kind: SimulatorInterfaceSelectionKind;
  target_ip: string | null;
  target_count: number;
  matched_target_count: number;
  matched_network: string | null;
  matching_interface_ids: string[];
}

interface ParsedIpv4Network {
  network: number;
  mask: number;
  prefix: number;
}

function parseIpv4(value: string): number | null {
  const octets = value.trim().split('.');
  if (octets.length !== 4) return null;
  let result = 0;
  for (const octet of octets) {
    if (!/^\d{1,3}$/.test(octet)) return null;
    const number = Number(octet);
    if (number > 255) return null;
    result = ((result << 8) | number) >>> 0;
  }
  return result;
}

function formatIpv4(value: number): string {
  return [24, 16, 8, 0].map((shift) => (value >>> shift) & 0xff).join('.');
}

function parseIpv4Network(value: string): ParsedIpv4Network | null {
  const parts = value.trim().split('/');
  if (parts.length !== 2) return null;
  const address = parseIpv4(parts[0]);
  const prefix = Number(parts[1]);
  if (address === null || !Number.isInteger(prefix) || prefix < 0 || prefix > 32) return null;
  const mask = prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
  return { network: (address & mask) >>> 0, mask, prefix };
}

function networkLabel(network: ParsedIpv4Network): string {
  return `${formatIpv4(network.network)}/${network.prefix}`;
}

/** Recommend the adapter whose configured IPv4 subnet contains the target device addresses. */
export function recommendSimulatorInterface(
  interfaces: SimulatorNetworkInterfaceInfo[],
  startIp: string,
  deviceIps: string[],
  currentInterfaceId: string,
): SimulatorInterfaceSelection {
  const available = interfaces.filter((item) => item.is_enabled && item.is_up);
  if (available.length === 0) {
    return {
      recommended_interface_id: '',
      kind: 'unavailable',
      target_ip: null,
      target_count: 0,
      matched_target_count: 0,
      matched_network: null,
      matching_interface_ids: [],
    };
  }

  const requestedIps = deviceIps.length > 0 ? deviceIps : [startIp];
  const targets = [...new Set(requestedIps.map((value) => value.trim()).filter(Boolean))]
    .map((value) => ({ value, address: parseIpv4(value) }))
    .filter((item): item is { value: string; address: number } => item.address !== null);
  const currentIsAvailable = available.some((item) => item.id === currentInterfaceId);

  if (targets.length === 0) {
    return {
      recommended_interface_id: currentIsAvailable ? currentInterfaceId : available[0].id,
      kind: 'invalid_target',
      target_ip: null,
      target_count: 0,
      matched_target_count: 0,
      matched_network: null,
      matching_interface_ids: [],
    };
  }

  const scores = available.map((item) => {
    const networks = item.ipv4_addresses
      .map(parseIpv4Network)
      .filter((network): network is ParsedIpv4Network => network !== null);
    let matchedTargetCount = 0;
    let firstTarget: string | null = null;
    let firstNetwork: string | null = null;
    for (const target of targets) {
      const match = networks.find((network) => (target.address & network.mask) >>> 0 === network.network);
      if (!match) continue;
      matchedTargetCount += 1;
      firstTarget ??= target.value;
      firstNetwork ??= networkLabel(match);
    }
    return { item, matchedTargetCount, firstTarget, firstNetwork };
  });

  const bestScore = Math.max(...scores.map((score) => score.matchedTargetCount));
  if (bestScore === 0) {
    return {
      recommended_interface_id: currentIsAvailable ? currentInterfaceId : available[0].id,
      kind: 'fallback',
      target_ip: targets[0].value,
      target_count: targets.length,
      matched_target_count: 0,
      matched_network: null,
      matching_interface_ids: [],
    };
  }

  const best = scores.filter((score) => score.matchedTargetCount === bestScore);
  const selected = best.find((score) => score.item.id === currentInterfaceId) ?? best[0];
  return {
    recommended_interface_id: selected.item.id,
    kind: best.length > 1 ? 'ambiguous' : 'matched',
    target_ip: selected.firstTarget,
    target_count: targets.length,
    matched_target_count: selected.matchedTargetCount,
    matched_network: selected.firstNetwork,
    matching_interface_ids: best.map((score) => score.item.id),
  };
}
