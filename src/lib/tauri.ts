import { invoke } from '@tauri-apps/api/core';

export interface DeployServer {
  id: string;
  enabled: boolean;
  name: string;
  host: string;
  port: number;
  user: string;
  password: string;
  remote_path: string;
}

export interface AppConfig {
  remote_paths: string[];
  target_versions: string[];
  local_path: string;
  interval_minutes: number;
  time_ranges: string[]; // Format "HH:mm-HH:mm" e.g. "05:00-09:00"
  file_extensions: string[];
  filename_includes: string[];
  
  deploy_enabled: boolean;
  servers: DeployServer[];
  
  // Legacy
  ssh_host: string;
  ssh_port: number;
  ssh_user: string;
  ssh_password: string;
  remote_linux_path: string;
  
  post_commands: string[];
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

export async function cancelScan(): Promise<void> {
  await invoke('cancel_scan');
}

export async function pauseScan(): Promise<void> {
  await invoke('pause_scan');
}

export async function resumeScan(): Promise<void> {
  await invoke('resume_scan');
}

export async function addSystemEvent(action: string, desc: string): Promise<void> {
  await invoke('add_system_event', { action, desc });
}

export interface HistoryEntry {
  id: string;
  timestamp: string;
  action_type: string;
  description: string;
  folder_name: string;
  source_path: string;
  target_path: string;
  copied_files_count: number;
  total_size: number;
  files: string[];
}

export interface HistoryStore {
  entries: HistoryEntry[];
}

export async function getHistory(): Promise<HistoryStore> {
  return await invoke('get_history');
}

export async function clearHistory(): Promise<void> {
  await invoke('clear_history');
}

export async function testSshConnection(server: DeployServer): Promise<string> {
  return await invoke('test_ssh_connection', { server });
}

export async function manualDeploy(server: DeployServer, postCommands: string[], localPath: string, remotePath: string): Promise<void> {
  await invoke('manual_deploy', { server, post_commands: postCommands, local_path: localPath, remote_path: remotePath });
}
