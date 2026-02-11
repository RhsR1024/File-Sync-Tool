import { invoke } from '@tauri-apps/api/core';

export interface AppConfig {
  remote_paths: string[];
  target_versions: string[];
  local_path: string;
  interval_minutes: number;
}

export interface ScanResult {
  scanned_paths: number;
  found_folders: string[];
  copied_folders: string[];
  errors: string[];
}

export async function getConfig(): Promise<AppConfig> {
  return await invoke('get_config');
}

export async function saveConfig(config: AppConfig): Promise<void> {
  await invoke('save_config_cmd', { config });
}

export async function scanNow(): Promise<ScanResult> {
  return await invoke('scan_now');
}
