import { invoke } from '@tauri-apps/api/core';

export interface CommandGroup {
  id: string;
  name: string;
  commands: string[];
}

export interface TaskServerBinding {
  server_id: string;
  command_group_ids: string[];
}

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

export interface MatchRule {
  type: 'VersionMatch' | 'DateMatch';
  value: string;
}

export interface ScanTask {
  id: string;
  enabled: boolean;
  name: string;
  remote_path: string;
  local_path: string | null;
  rule: MatchRule;
  /** Per-server deployment bindings with command groups. */
  server_bindings: TaskServerBinding[];
}

export interface AppConfig {
  tasks: ScanTask[];

  local_path: string;
  interval_minutes: number;
  time_ranges: string[]; // Format "HH:mm-HH:mm" e.g. "05:00-09:00"
  file_extensions: string[];
  filename_includes: string[];

  deploy_enabled: boolean;
  servers: DeployServer[];

  /** Named command groups. */
  command_groups: CommandGroup[];

  /** Seconds to wait before copying to verify files are fully written. Minimum: 60. */
  stability_check_secs: number;

  /** If a file was modified within the last N minutes, it must pass the stability wait. Minimum: 3. */
  recent_file_guard_mins: number;

  /** One switch: launch on startup + auto start scheduler scan after app launch */
  launch_and_auto_scan: boolean;

  /** When enabled, clicking the window close button hides to tray instead of exiting. */
  close_to_tray: boolean;

  /** Maximum number of log lines to display in the console. Default: 200. */
  max_log_lines: number;

  /** Copy buffer size in KB. Controls read/write chunk size when copying files. Default: 4096 (4 MB). */
  copy_buffer_size_kb: number;

  /** Maximum number of task records to persist and display. Default: 100. */
  max_task_records: number;
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

export async function skipCurrentCopy(): Promise<void> {
  await invoke('skip_current_copy');
}

export async function removeFromScanQueue(folder: string): Promise<void> {
  await invoke('remove_from_scan_queue', { folder });
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
  await invoke('manual_deploy', { server, postCommands, localPath, remotePath });
}

export interface ManualCopyQueueAck {
  folder_name: string;
  source_path: string;
  local_path: string;
  queued_ahead: number;
}

export interface ManualCopyPreview {
  folder_name: string;
  source_path: string;
  local_path: string;
  resolved_target_path: string;
  source_kind: 'file' | 'directory';
  target_exists: boolean;
}

export async function temporaryCopy(
  sourcePath: string,
  targetRootPath: string,
  overwriteExisting = false,
  fileExtensions: string[] = [],
  filenameIncludes: string[] = [],
): Promise<void> {
  await invoke('temporary_copy', { sourcePath, targetRootPath, overwriteExisting, fileExtensions, filenameIncludes });
}

export async function queueTemporaryCopy(
  sourcePath: string,
  targetRootPath: string,
  overwriteExisting = false,
  fileExtensions: string[] = [],
  filenameIncludes: string[] = [],
): Promise<ManualCopyQueueAck> {
  return await invoke('queue_temporary_copy', { sourcePath, targetRootPath, overwriteExisting, fileExtensions, filenameIncludes });
}

export async function previewTemporaryCopy(sourcePath: string, targetRootPath: string): Promise<ManualCopyPreview> {
  return await invoke('preview_temporary_copy', { sourcePath, targetRootPath });
}

export async function getAppPaths(): Promise<[string, string]> {
  return await invoke('get_app_paths');
}

export async function openPathParent(path: string): Promise<void> {
  await invoke('open_path_parent', { path });
}

export async function openDirectory(): Promise<string | null> {
  return await invoke('open_directory');
}

export async function saveTextFile(
  content: string,
  defaultFileName: string,
  filterName: string,
  extensions: string[],
): Promise<string | null> {
  return await invoke('save_text_file', {
    content,
    defaultFileName,
    filterName,
    extensions,
  });
}

// Framework password management types
export interface FrameworkPasswordResult {
  ip: string;
  success: boolean;
  message: string;
  /** If success is false, indicates where the failure occurred. */
  failedAt?: 'login' | 'changePasswd';
}

export interface ApplianceSshResult {
  ip: string;
  success: boolean;
  message: string;
  previousEnable?: number;
  currentEnable?: number;
  port?: number;
  whitelistSourceIp?: string;
  whitelistApplied?: boolean;
}

export interface EnableApplianceSshRequest {
  ips: string[];
  sshUsername?: string;
  sshPassword?: string;
  addWhitelistRule: boolean;
}

// Internal: Login API response (used by backend)
interface LoginResponse {
  code: number;
  message: string;
  data?: {
    firstLogin: boolean;
    token: string;
  };
}

// Internal: ChangePasswd API response (used by backend)
interface ChangePasswdResponse {
  code: number;
  message: string;
}

// ─── Code Count (code statistics) ─────────────────────────────

export interface CodeCountFileStats {
  filePath: string;
  codeAdded: number;
  codeDeleted: number;
  codeModified: number;
  commentAdded: number;
  commentDeleted: number;
  commentModified: number;
}

export interface CodeCountSummary {
  codeAdded: number;
  codeDeleted: number;
  codeModified: number;
  commentAdded: number;
  commentDeleted: number;
  commentModified: number;
}

export interface CodeCountOperationSummary {
  addedTotal: number;
  deletedTotal: number;
  modifiedTotal: number;
  changedTotal: number;
}

export interface CodeCountResult {
  files: CodeCountFileStats[];
  summary: CodeCountSummary;
  operationSummary: CodeCountOperationSummary;
  fileTypeSummary: Record<string, CodeCountSummary>;
}

export interface CodeCountProgress {
  phase: string;
  currentFile: string;
  processedFiles: number;
  totalFiles: number;
  percent: number;
}

export interface CodeCountScopeTreeNode {
  key: string;
  label: string;
  kind: 'directory' | 'file';
  children: CodeCountScopeTreeNode[];
}

export interface UiState {
  logs: unknown[];
  task_records: unknown[];
}

export async function saveUiState(logs: unknown[], taskRecords: unknown[]): Promise<void> {
  await invoke('save_ui_state', { logs, taskRecords });
}

export async function loadUiState(): Promise<UiState> {
  return await invoke('load_ui_state');
}

export async function confirmQuit(): Promise<void> {
  await invoke('confirm_quit');
}

export async function codeCountAnalyze(
  oldPath: string,
  newPath: string,
  includedOldPaths?: string[],
  includedNewPaths?: string[],
  includeExtensions?: string[],
  excludeExtensions?: string[],
  includeVcsDirs?: boolean,
): Promise<CodeCountResult> {
  return await invoke<CodeCountResult>('code_count_analyze', {
    oldPath,
    newPath,
    includedOldPaths,
    includedNewPaths,
    includeExtensions,
    excludeExtensions,
    includeVcsDirs,
  });
}

export async function codeCountListScopeTree(
  paths: string[],
  includeExtensions?: string[],
  excludeExtensions?: string[],
  includeVcsDirs?: boolean,
): Promise<CodeCountScopeTreeNode[]> {
  return await invoke<CodeCountScopeTreeNode[]>('code_count_list_scope_tree', {
    paths,
    includeExtensions,
    excludeExtensions,
    includeVcsDirs,
  });
}

/**
 * Change framework password for specified IPs.
 * @param ips - Array of IP addresses to change password for
 * @returns Array of results indicating success/failure for each IP
 */
export async function changeFrameworkPassword(
  ips: string[],
  oldPassword?: string,
  newPassword?: string,
): Promise<FrameworkPasswordResult[]> {
  return await invoke<FrameworkPasswordResult[]>('change_framework_password', {
    ips,
    oldPassword,
    newPassword,
  });
}

export async function enableApplianceSsh(request: EnableApplianceSshRequest): Promise<ApplianceSshResult[]> {
  return await invoke<ApplianceSshResult[]>('enable_appliance_ssh', { request });
}

// ─── Network Tools ─────────────────────────────────────

export interface PingResult {
  ip: string;
  alive: boolean;
  latencyMs: number | null;
}

export interface PingScanRequest {
  prefix: string;
  start: number;
  end: number;
  timeoutMs: number;
}

export interface TcpConnectionStats {
  total: number;
  byState: { state: string; count: number }[];
  byRemoteIp: { ip: string; count: number }[];
  byRemotePort: { port: number; name: string; count: number }[];
}

export interface PortTestRequest {
  host: string;
  ports: number[];
  timeoutMs: number;
}

export interface SinglePortResult {
  port: number;
  open: boolean;
  latencyMs: number | null;
  name: string;
}

export interface PortTestResult {
  host: string;
  resolvedIp: string | null;
  results: SinglePortResult[];
}

export interface WolRequest {
  mac: string;
  broadcastIp?: string;
  port?: number;
}

export interface WolResult {
  mac: string;
  success: boolean;
  message: string;
}

export interface PortPreset {
  name: string;
  ports: string;
}

export interface WolDevice {
  name: string;
  mac: string;
  broadcast: string;
  port: number;
}

export async function pingScan(request: PingScanRequest): Promise<void> {
  await invoke('ping_scan', { request });
}

export async function cancelPingScan(): Promise<void> {
  await invoke('cancel_ping_scan');
}

export async function getTcpConnections(): Promise<TcpConnectionStats> {
  return await invoke('get_tcp_connections');
}

export async function testPorts(request: PortTestRequest): Promise<PortTestResult> {
  return await invoke('test_ports', { request });
}

export async function sendWol(request: WolRequest): Promise<WolResult> {
  return await invoke('send_wol', { request });
}
