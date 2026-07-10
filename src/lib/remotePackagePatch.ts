import type {
  EnableApplianceSshRequest,
  InternalLayer,
  PackageEntry,
  PackageInventory,
  RemoteSshConfig,
} from './tauri';

export const REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT = 23333;

function isValidIpv4Address(value: string): boolean {
  const parts = value.split('.');
  return (
    parts.length === 4 &&
    parts.every((part) => /^\d+$/.test(part) && Number(part) >= 0 && Number(part) <= 255)
  );
}

export function resolveRemotePackagePatchSshPort(value: unknown): number {
  const port = typeof value === 'number' ? value : Number(value);
  if (Number.isInteger(port) && port > 0 && port <= 65535) return port;
  return REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT;
}

export function shouldAttemptRemotePackagePatchAutoEnable(error: string): boolean {
  const lower = error.toLowerCase();
  return lower.includes('tcp connect failed') || lower.includes('ssh handshake failed');
}

export function buildRemotePackagePatchEnableSshRequest(config: RemoteSshConfig): EnableApplianceSshRequest | null {
  const host = config.host.trim();
  if (!isValidIpv4Address(host)) return null;

  return {
    targets: [{ ip: host }],
    applianceVersion: 'componentized',
    whitelistScope: 'allTcp',
    sshUsername: config.username.trim(),
    sshPassword: config.auth.kind === 'password' ? config.auth.password : '',
    addWhitelistRule: false,
  };
}

export function replacementName(pathOrName: string): string {
  const normalized = pathOrName.replace(/\\/g, '/');
  const parts = normalized.split('/').filter(Boolean);
  return parts.at(-1) ?? '';
}

export function defaultPatchedPath(packagePath: string): string {
  if (packagePath.endsWith('.tar.gz')) {
    return `${packagePath.slice(0, -'.tar.gz'.length)}.patched.tar.gz`;
  }
  return `${packagePath}.patched.tar.gz`;
}

export function layerKey(layer: InternalLayer | null | undefined): string {
  if (!layer) return 'auto';
  return layer.kind === 'zst' ? `zst:${layer.zstPath}` : 'middle';
}

export function targetCandidates(inventory: PackageInventory, fileName: string): PackageEntry[] {
  const expected = replacementName(fileName);
  if (!expected) return [];
  return inventory.entries
    .filter((entry) => entry.kind === 'file' && replacementName(entry.path) === expected)
    .sort((left, right) => {
      const leftLayer = layerKey(left.layer);
      const rightLayer = layerKey(right.layer);
      return leftLayer.localeCompare(rightLayer) || left.path.localeCompare(right.path);
    });
}

export function composeInternalTargetPath(directory: string, fileName: string): string {
  const dir = directory.trim().replace(/\\/g, '/').replace(/\/+$/g, '');
  const file = fileName.trim().replace(/\\/g, '/').replace(/^\/+/g, '');
  return dir ? `${dir}/${file}` : file;
}

export type TargetPathErrorCode = 'required' | 'absolute' | 'trailingSlash' | 'parentSegment';

/** Returns an error code for the UI to localize, or null when the path is valid. */
export function validateInternalTargetPath(path: string): TargetPathErrorCode | null {
  const value = path.trim().replace(/\\/g, '/');
  if (!value) return 'required';
  if (value.startsWith('/')) return 'absolute';
  if (value.endsWith('/')) return 'trailingSlash';
  if (value.split('/').some((part) => part === '..')) return 'parentSegment';
  return null;
}

export function orderedStages(): string[] {
  return [
    'upload',
    'preflight',
    'unpack_outer',
    'extract_middle',
    'resolve_target',
    'extract_inner',
    'replace_member',
    'update_md5',
    'repack_inner',
    'repack_middle',
    'compress_outer',
    'verify',
    'backup_overwrite',
    'finalize',
    'cleanup',
  ];
}

export function visibleStages(options: { overwrite: boolean; layer: InternalLayer | null }): string[] {
  return orderedStages().filter((stage) => {
    if (stage === 'backup_overwrite') return options.overwrite;
    if (stage === 'finalize') return !options.overwrite;
    if (stage === 'extract_inner' || stage === 'repack_inner') {
      return options.layer === null || options.layer.kind === 'zst';
    }
    return true;
  });
}

export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let amount = value;
  let unitIndex = 0;
  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }
  return `${amount >= 10 || unitIndex === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[unitIndex]}`;
}
