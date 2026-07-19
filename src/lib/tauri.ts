import { invoke } from '@tauri-apps/api/core';

import type {
  ClipboardGroup,
  ClipboardItem,
  ClipboardListQuery,
  ClipboardListResult,
  ClipboardSettings,
  ClipboardStats,
  FilePathStatus,
} from './clipboardTypes';
import type {
  ClipboardImagePreviewPayload,
  ClipboardTextPreviewPayload,
} from './clipboardPreviewHelpers';
import type { DeviceSimulatorSettings } from './deviceSimulator';

export type {
  ClipboardGroup,
  ClipboardSearchFilters,
  ClipboardSearchPayload,
  FilePathStatus,
} from './clipboardTypes';

export interface CommandGroup {
  id: string;
  name: string;
  commands: string[];
}

export type OnFailure = 'continue' | 'abort';

export interface LocalCommandGroup {
  id: string;
  name: string;
  commands: string[];
  on_failure: OnFailure;
}

export interface LocalScriptBinding {
  command_group_ids: string[];
}

export type PostCopyExecutionOrder = 'local_first' | 'remote_first' | 'parallel';

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
  /** SSH TCP connect timeout in seconds. Default: 30. */
  ssh_timeout_secs: number;
}

export interface MatchRule {
  type: 'VersionMatch' | 'DateMatch';
  value: string;
}

export type DiskCleanupLinuxMode = 'componentized' | 'mainline';

export type CopyMode = 'built_in' | 'windows_shell';

export interface ScanTask {
  id: string;
  enabled: boolean;
  name: string;
  remote_path: string;
  local_path: string | null;
  rule: MatchRule;
  /** Per-server deployment bindings with command groups. */
  server_bindings: TaskServerBinding[];
  local_script_binding: LocalScriptBinding | null;
  post_copy_execution_order: PostCopyExecutionOrder;
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

  /** Named local command groups for post-copy local script execution. */
  local_command_groups: LocalCommandGroup[];

  /** Seconds to wait before copying to verify files are fully written. Minimum: 60. */
  stability_check_secs: number;

  /** If a file was modified within the last N minutes, it must pass the stability wait. Minimum: 3. */
  recent_file_guard_mins: number;

  /** One switch: launch on startup + auto start scheduler scan after app launch */
  launch_and_auto_scan: boolean;

  /** Launch the app with the system and automatically restore file share if saved settings allow it. */
  launch_and_auto_start_file_share: boolean;

  /** When enabled, clicking the window close button hides to tray instead of exiting. */
  close_to_tray: boolean;

  /** Show native system notifications for scanned sync task milestones. Default: true. */
  sync_task_notifications_enabled: boolean;

  /** Maximum number of log lines to display in the console. Default: 200. */
  max_log_lines: number;

  /** Copy buffer size in KB. Controls read/write chunk size when copying files. Default: 4096 (4 MB). */
  copy_buffer_size_kb: number;

  /** Copy implementation used after filters and stability checks. Default: built_in. */
  copy_mode: CopyMode;

  /** Maximum number of task records to persist and display. Default: 100. */
  max_task_records: number;

  /** HTTP request timeout in seconds for the appliance SSH API. Default: 5. */
  appliance_ssh_api_timeout_secs: number;

  /** HTTP request timeout in seconds for the framework password API. Default: 5. */
  framework_password_api_timeout_secs: number;

  /** HTTP request timeout in seconds for disk cache cleanup API. Default: 5. */
  disk_cleanup_http_timeout_secs: number;

  /** Linux disk source mode in the cache cleanup tool. Default: "componentized". */
  disk_cleanup_linux_mode: DiskCleanupLinuxMode;

  /** Update server URL used by the in-app updater. */
  update_server_url: string;

  /** Whether startup auto-checks should show the update dialog. */
  notify_on_new_version: boolean;

  /** RFC3339 timestamp of the last successful update check. */
  last_update_check_at: string | null;

  /** Downloaded update waiting to be applied on restart. */
  pending_update: PendingUpdate | null;

  /** Clipboard manager settings mirrored from Rust AppConfig. */
  clipboard: ClipboardSettings;

  /** Video device simulator application-domain preferences. */
  device_simulator: DeviceSimulatorSettings;
}

export interface SyncConfigPatch extends Pick<
  AppConfig,
  | 'tasks'
  | 'local_path'
  | 'interval_minutes'
  | 'time_ranges'
  | 'file_extensions'
  | 'filename_includes'
  | 'deploy_enabled'
  | 'servers'
  | 'command_groups'
  | 'local_command_groups'
  | 'stability_check_secs'
  | 'recent_file_guard_mins'
  | 'copy_buffer_size_kb'
  | 'copy_mode'
> {}

export interface AppDomainConfigPatch extends Pick<
  AppConfig,
  | 'launch_and_auto_scan'
  | 'launch_and_auto_start_file_share'
  | 'close_to_tray'
  | 'sync_task_notifications_enabled'
  | 'max_log_lines'
  | 'max_task_records'
  | 'appliance_ssh_api_timeout_secs'
  | 'framework_password_api_timeout_secs'
  | 'disk_cleanup_http_timeout_secs'
  | 'disk_cleanup_linux_mode'
  | 'update_server_url'
  | 'notify_on_new_version'
  | 'clipboard'
  | 'device_simulator'
> {}

export interface ManifestVersion {
  version: string;
  url: string;
  sha256: string;
  released_at: string;
  changelog: string[];
}

export interface Manifest {
  latest: string;
  versions: ManifestVersion[];
}

export interface PendingUpdate {
  target_version: string;
  temp_path: string;
  target_file_name: string;
  sha256: string;
  downloaded_at: string;
}

export interface UpdateState {
  current: string;
  server_url: string;
  manifest: Manifest | null;
  has_update: boolean;
  last_checked_at: string | null;
  pending_update: PendingUpdate | null;
  debug_build: boolean;
}

export interface DownloadProgress {
  downloaded: number;
  total: number | null;
  speed_bps: number;
}

export interface DownloadCompletePayload {
  version: string;
  temp_path: string;
  sha256_ok: boolean;
  error: string | null;
}

export interface UpdateCheckResult {
  has_update: boolean;
  current: string;
  latest: string | null;
  manifest: Manifest | null;
}

export interface TestServerResult {
  ok: boolean;
  status: number | null;
  error: string | null;
}

export type TaskSourceType = 'scheduled' | 'manual';

export type TaskTriggerSource = 'scheduled' | 'manual' | 'recovery';

export type TaskRunType = 'copy_and_deploy' | 'deploy_retry' | 'manual_deploy';

export type TaskSummaryStatus =
  | 'queued'
  | 'copying'
  | 'paused'
  | 'cancelling'
  | 'copy_completed'
  | 'local_executing'
  | 'deploying'
  | 'partial_failed'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

export type CopyState = 'pending' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted';

export type LocalExecState = 'not_started' | 'running' | 'completed' | 'partial_failed' | 'failed' | 'cancelled' | 'interrupted';

export type DeployState =
  | 'not_started'
  | 'pending'
  | 'running'
  | 'completed'
  | 'partial_failed'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

export type AttemptStatus = 'running' | 'success' | 'failed' | 'cancelled' | 'interrupted';

export type DeployStage = 'pending' | 'connecting' | 'uploading' | 'executing_commands' | 'done';

export interface TaskRunHandle {
  task_group_id: string;
  run_id: string;
}

export interface StartManualCopyTaskRequest {
  source_path: string;
  target_root_path: string;
  overwrite_existing?: boolean;
  file_extensions?: string[];
  filename_includes?: string[];
}

export interface StartManualDeployTaskBindingRequest {
  server_id: string;
  command_group_ids: string[];
}

export interface StartManualDeployTaskRequest {
  task_group_id: string | null;
  display_name?: string;
  folder_name?: string;
  local_path: string;
  remote_path: string;
  bindings: StartManualDeployTaskBindingRequest[];
}

export interface DeployAttempt {
  attempt_id: string;
  task_group_id: string;
  run_id: string;
  server_id: string;
  server_name: string;
  attempt_no: number;
  trigger_source: TaskTriggerSource;
  stage: DeployStage;
  status: AttemptStatus;
  remote_target: string | null;
  started_at: string;
  finished_at: string | null;
  elapsed_seconds: number;
  progress_percentage: number | null;
  error_phase: DeployStage | null;
  error_message: string | null;
  last_log_excerpt: string | null;
}

export interface TaskRun {
  run_id: string;
  task_group_id: string;
  run_type: TaskRunType;
  trigger_source: TaskTriggerSource;
  started_at: string;
  finished_at: string | null;
  copy_phase: CopyState;
  local_exec_phase: LocalExecState;
  deploy_phase: DeployState;
  deploy_attempts: DeployAttempt[];
  attempt_ids: string[];
}

export interface ServerRollup {
  server_id: string;
  server_name: string;
  latest_status: AttemptStatus;
  latest_attempt_id: string | null;
  success_count: number;
  failure_count: number;
  last_error_message: string | null;
  attempt_ids: string[];
}

export interface TaskGroupListItem {
  task_group_id: string;
  merge_key: string;
  task_config_id: string | null;
  display_name: string;
  folder_name: string;
  source_path: string;
  local_target_path: string;
  copy_status: CopyState;
  local_exec_status: LocalExecState;
  deploy_status: DeployState;
  summary_status: TaskSummaryStatus;
  started_at: string;
  finished_at: string | null;
  elapsed_seconds: number;
  latest_run_id: string | null;
  had_failures: boolean;
  server_rollups: ServerRollup[];
}

export interface TaskGroup extends TaskGroupListItem {
  source_type: TaskSourceType;
  runs: TaskRun[];
}

export interface TaskGroupsSnapshot {
  groups: TaskGroupListItem[];
}

export interface TaskGroupDetailSnapshot {
  task_group_id: string;
  group: TaskGroup;
}

export interface TaskLogEntry {
  task_group_id: string | null;
  run_id: string | null;
  server_id: string | null;
  server_name: string | null;
  level: 'info' | 'success' | 'warn' | 'error' | 'command' | string;
  message: string;
  timestamp: string;
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

// 前端存活标记：主窗口挂载后写入 app.log，用于区分“webview 没加载”与“窗口没显示”。
export async function markFrontendReady(label: string): Promise<void> {
  return await invoke('mark_frontend_ready', { label });
}

export async function saveConfig(config: AppConfig): Promise<void> {
  await invoke('save_config_cmd', { config });
}

export async function updateSyncConfig(patch: SyncConfigPatch): Promise<void> {
  await invoke('update_sync_config', { patch });
}

export async function updateAppConfig(patch: AppDomainConfigPatch): Promise<void> {
  await invoke('update_app_config', { patch });
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

export async function testSshConnection(server: DeployServer): Promise<string> {
  return await invoke('test_ssh_connection', { server });
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
  /** Seconds since the source file was last modified; null for directories or unreadable mtime. */
  source_modified_secs_ago: number | null;
}

export async function queueTemporaryCopy(
  sourcePath: string,
  targetRootPath: string,
  overwriteExisting = false,
  fileExtensions: string[] = [],
  filenameIncludes: string[] = [],
  skipStabilityCheck = false,
): Promise<ManualCopyQueueAck> {
  return await invoke('queue_temporary_copy', { sourcePath, targetRootPath, overwriteExisting, fileExtensions, filenameIncludes, skipStabilityCheck });
}

export async function previewTemporaryCopy(sourcePath: string, targetRootPath: string): Promise<ManualCopyPreview> {
  return await invoke('preview_temporary_copy', { sourcePath, targetRootPath });
}

export async function listTaskGroups(): Promise<TaskGroupListItem[]> {
  return await invoke('list_task_groups');
}

export async function getTaskGroupDetail(taskGroupId: string): Promise<TaskGroup> {
  return await invoke('get_task_group_detail', { taskGroupId });
}

export async function clearTaskGroup(taskGroupId: string): Promise<void> {
  await invoke('clear_task_group', { taskGroupId });
}

export async function clearTaskGroups(): Promise<void> {
  await invoke('clear_task_groups');
}

export async function cancelTaskRun(taskGroupId: string, runId: string): Promise<void> {
  await invoke('cancel_task_run', { taskGroupId, runId });
}

export async function pauseTaskRun(taskGroupId: string, runId: string): Promise<void> {
  await invoke('pause_task_run', { taskGroupId, runId });
}

export async function resumeTaskRun(taskGroupId: string, runId: string): Promise<void> {
  await invoke('resume_task_run', { taskGroupId, runId });
}

export async function retryTaskGroupDeploy(taskGroupId: string): Promise<TaskRunHandle> {
  return await invoke('retry_task_group_deploy', { taskGroupId });
}

export async function startManualCopyTask(request: StartManualCopyTaskRequest): Promise<TaskRunHandle> {
  return await invoke('start_manual_copy_task', { request });
}

export async function startManualDeployTask(request: StartManualDeployTaskRequest): Promise<TaskRunHandle> {
  return await invoke('start_manual_deploy_task', { request });
}

export async function getAppPaths(): Promise<[string, string]> {
  return await invoke('get_app_paths');
}

export async function getCustomDataDir(): Promise<string> {
  return await invoke('get_custom_data_dir');
}

export async function setCustomDataDir(path: string): Promise<void> {
  await invoke('set_custom_data_dir', { path });
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
  jumpHost?: string;
}

export interface ApplianceSshTarget {
  ip: string;
  jumpHost?: string;
}

export type ApplianceSshApiVersion = 'componentized' | 'mainline';
export type ApplianceSshWhitelistScope = 'allTcp' | 'sshOnly';

export interface EnableApplianceSshRequest {
  targets: ApplianceSshTarget[];
  applianceVersion: ApplianceSshApiVersion;
  whitelistScope: ApplianceSshWhitelistScope;
  sshUsername?: string;
  sshPassword?: string;
  addWhitelistRule: boolean;
  whitelistCidr?: string;
  jumpHostUseSeparateCreds?: boolean;
  jumpHostUsername?: string;
  jumpHostPassword?: string;
  jumpHostSshPort?: number;
}

// Internal: Login API response (used by backend)
interface _LoginResponse {
  code: number;
  message: string;
  data?: {
    firstLogin: boolean;
    token: string;
  };
}

// Internal: ChangePasswd API response (used by backend)
interface _ChangePasswdResponse {
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
}

export async function saveUiState(logs: unknown[]): Promise<void> {
  await invoke('save_ui_state', { logs });
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

export async function codeCountCancel(): Promise<void> {
  await invoke('code_count_cancel');
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

// Remote Package Patch
export type RemoteAuth = { kind: 'password'; password: string };

export interface RemoteSshConfig {
  host: string;
  port: number;
  username: string;
  auth: RemoteAuth;
}

export interface RemoteDirEntry {
  name: string;
  path: string;
  kind: 'dir' | 'file' | 'symlink' | 'other';
  size: number;
  modifiedMs: number | null;
}

export interface RemoteDirListing {
  path: string;
  entries: RemoteDirEntry[];
}

export type InternalLayer =
  | { kind: 'middle' }
  | { kind: 'zst'; zstPath: string };

export interface PackageEntry {
  layer: InternalLayer;
  path: string;
  kind: 'file' | 'dir' | 'symlink' | 'other';
  size: number;
  permsText: string;
  ownerText: string;
  mtimeText: string;
}

export interface PackageInventory {
  packagePath: string;
  middleTarPath: string;
  entries: PackageEntry[];
}

export interface PickedLocalFile {
  path: string;
  name: string;
  size: number;
}

export type PatchOutputPolicy =
  | { mode: 'newFile'; outputPath: string }
  | { mode: 'overwrite' };

export interface PackagePatchRequest {
  config: RemoteSshConfig;
  packagePath: string;
  replacementLocalPath: string;
  targetInternalPath: string;
  targetLayer: InternalLayer | null;
  output: PatchOutputPolicy;
}

export interface PackagePatchResult {
  outputPath: string;
  backupPath: string | null;
  targetMd5: string;
  workdir: string;
  updatedManifests: string[];
}

export interface RemotePackagePatchEvent {
  kind: 'stage' | 'log' | 'result' | 'uploadProgress';
  stage?: string;
  level?: 'info' | 'warn' | 'error' | string;
  message?: string;
  key?: string;
  value?: string;
  sent?: number;
  total?: number;
}

export const remotePackagePatchApi = {
  testConnection: (config: RemoteSshConfig) =>
    invoke<string>('remote_package_test_connection', { config }),
  listDir: (config: RemoteSshConfig, path: string) =>
    invoke<RemoteDirListing>('remote_package_list_dir', { config, path }),
  pickLocalFile: () => invoke<PickedLocalFile | null>('remote_package_pick_local_file'),
  scanPackage: (config: RemoteSshConfig, packagePath: string) =>
    invoke<PackageInventory>('remote_package_scan_package', { config, packagePath }),
  startPatch: (request: PackagePatchRequest) =>
    invoke<PackagePatchResult>('remote_package_start_patch', { request }),
};

// Disk Cache Cleanup
export interface DiskServerItem {
  serverName: string;
  serverIp: string;
  role: string;
  serial: string;
  haType: number;
  serverCode: number;
}

export interface Wwn {
  wwn: string;
  blockSize: number;
}

export interface DiskInfoItem {
  storageId: string;
  storageType: number;
  slot: number;
  enclosureIndex: number;
  storageStatus: number;
  totalCapacity: number;
  usage: number;
  deviceName: string;
  worldWideNameList: Wwn[];
}

export interface WindowsPartitionItem {
  partitionSeq: number;
  partitionGUID: string;
  partitionOffset: string;
  capacity: number;
  partitionStatus: number;
  usage: number;
}

export interface WindowsDiskItem {
  diskId: string;
  diskNumber: number;
  diskName: string;
  totalCapacity: number;
  partitionList: WindowsPartitionItem[];
}

export interface IpsanItem {
  IPSANId: string;
  IPSANName: string;
  IPSANType: number;
  IPSANIp: string;
  IPSANStatus: number;
  totalCapacity: number;
  usage: number;
}

export interface IpsanResourceGroupMemberItem {
  IPSANId: string;
  IPSANName: string;
  IPSANIp: string;
  IPSANStatus: number;
  capacity: number;
}

export interface IpsanResourceGroupItem {
  groupId: string;
  groupName: string;
  groupStatus: number;
  totalCapacity: number;
  usage: number;
  resourceInfoList: IpsanResourceGroupMemberItem[];
}

export interface CacheKeyCheckResult {
  present_keys: string[];
  redis_available: boolean;
  error: string | null;
}

export interface CacheKeyContentEntry {
  key: string;
  value_type: string;
  preview: string;
  full_value: string;
  truncated: boolean;
}

export interface CacheKeyContentResult {
  entries: CacheKeyContentEntry[];
  redis_available: boolean;
  error: string | null;
}

export interface CacheKeyDeleteResult {
  deleted_count: number;
  redis_available: boolean;
  error: string | null;
}

export async function diskCleanupListLinuxServers(
  host: string,
  timeoutSecs: number,
): Promise<DiskServerItem[]> {
  return await invoke<DiskServerItem[]>('disk_cleanup_list_linux_servers', {
    host,
    timeoutSecs,
  });
}

export async function diskCleanupListMainlineServers(
  host: string,
  timeoutSecs: number,
): Promise<DiskServerItem[]> {
  return await invoke<DiskServerItem[]>('disk_cleanup_list_mainline_servers', {
    host,
    timeoutSecs,
  });
}

export async function diskCleanupListLinuxDisks(
  host: string,
  serverIp: string,
  timeoutSecs: number,
): Promise<DiskInfoItem[]> {
  return await invoke<DiskInfoItem[]>('disk_cleanup_list_linux_disks', {
    host,
    serverIp,
    timeoutSecs,
  });
}

export async function diskCleanupListWindowsDisks(
  host: string,
  timeoutSecs: number,
): Promise<WindowsDiskItem[]> {
  return await invoke<WindowsDiskItem[]>('disk_cleanup_list_windows_disks', {
    host,
    timeoutSecs,
  });
}

export async function diskCleanupListIpsans(
  host: string,
  timeoutSecs: number,
): Promise<IpsanItem[]> {
  return await invoke<IpsanItem[]>('disk_cleanup_list_ipsans', {
    host,
    timeoutSecs,
  });
}

export async function diskCleanupListIpsanResourceGroups(
  host: string,
  timeoutSecs: number,
): Promise<IpsanResourceGroupItem[]> {
  return await invoke<IpsanResourceGroupItem[]>('disk_cleanup_list_ipsan_resource_groups', {
    host,
    timeoutSecs,
  });
}

export async function diskCleanupCheckCacheKeys(
  host: string,
  keys: string[],
): Promise<CacheKeyCheckResult> {
  return await invoke<CacheKeyCheckResult>('disk_cleanup_check_cache_keys', {
    host,
    keys,
  });
}

export async function diskCleanupGetCacheKeyContents(
  host: string,
  keys: string[],
): Promise<CacheKeyContentResult> {
  return await invoke<CacheKeyContentResult>('disk_cleanup_get_cache_key_contents', {
    host,
    keys,
  });
}

export async function diskCleanupDeleteCacheKeys(
  host: string,
  keys: string[],
): Promise<CacheKeyDeleteResult> {
  return await invoke<CacheKeyDeleteResult>('disk_cleanup_delete_cache_keys', {
    host,
    keys,
  });
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

export async function cancelPortTest(): Promise<void> {
  await invoke('cancel_port_test');
}

export async function sendWol(request: WolRequest): Promise<WolResult> {
  return await invoke('send_wol', { request });
}

export type MonitorControlFeature = 'brightness' | 'contrast';

export interface DisplayControlMonitor {
  id: string;
  index: number;
  name: string;
  is_primary: boolean;
  is_internal: boolean;
  backend: 'ddc_ci' | 'wmi' | string;
  brightness: number | null;
  brightness_min: number;
  brightness_max: number;
  brightness_supported: boolean;
  contrast: number | null;
  contrast_min: number;
  contrast_max: number;
  contrast_supported: boolean;
}

export interface MonitorControlSetRequest {
  monitor_id: string;
  feature: MonitorControlFeature;
  value: number;
}

export const monitorControlApi = {
  listMonitors(): Promise<DisplayControlMonitor[]> {
    return invoke<DisplayControlMonitor[]>('monitor_control_list');
  },
  setFeature(request: MonitorControlSetRequest): Promise<void> {
    return invoke<void>('monitor_control_set', { request });
  },
};

// ─── Screen Share ─────────────────────────────────────

export type ScreenShareBackendMode = 'auto' | 'wgc' | 'dxgi';

export interface ScreenShareConfig {
  port: number;
  username: string | null;
  password: string | null;
  monitor_index: number;
  quality: number;
  fps: number;
  show_cursor: boolean;
  capture_backend_mode?: ScreenShareBackendMode;
  /** Bind address: "0.0.0.0" for all interfaces, or a specific IP. */
  bind_address?: string | null;
}

export interface NetworkInterfaceInfo {
  name: string;
  ip: string;
}

export interface MonitorInfo {
  index: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

export type ScreenShareCaptureIssue = 'retrying' | 'privacy_mode_or_display_off';

export interface ScreenShareStatus {
  is_active: boolean;
  viewer_count: number;
  connection_count: number;
  fps_actual: number;
  bitrate_kbps: number;
  uptime_secs: number;
  server_url: string;
  all_urls: string[];
  connected_ips: string[];
  capture_paused: boolean;
  capture_issue: ScreenShareCaptureIssue | null;
  interaction_connected_count: number;
  annotation_count: number;
  view_mode: 'live' | 'frozen';
}

export async function screenShareListMonitors(): Promise<MonitorInfo[]> {
  return await invoke<MonitorInfo[]>('screen_share_list_monitors');
}

export async function screenShareListInterfaces(): Promise<NetworkInterfaceInfo[]> {
  return await invoke<NetworkInterfaceInfo[]>('screen_share_list_interfaces');
}

export async function screenShareStart(config: ScreenShareConfig): Promise<string> {
  return await invoke<string>('screen_share_start', { config });
}

export async function screenShareStop(): Promise<void> {
  await invoke('screen_share_stop');
}

export async function screenShareGetStatus(): Promise<ScreenShareStatus> {
  return await invoke<ScreenShareStatus>('screen_share_get_status');
}

export async function screenShareClearAnnotations(): Promise<void> {
  await invoke('screen_share_clear_annotations');
}

export async function screenShareOpenLocalPreview(): Promise<void> {
  await invoke('screen_share_open_local_preview');
}

export async function screenShareCloseLocalPreview(): Promise<void> {
  await invoke('screen_share_close_local_preview');
}

// ─── File Share ───────────────────────────────────────────

export interface SharedDir {
  alias: string;
  path: string;
}

export type FileSharePermissionPreset = 'read_only' | 'read_write' | 'custom';
export type FileShareDeleteMode = 'recycle_bin' | 'permanent';
export type FileShareIpFilterMode = 'off' | 'whitelist' | 'blacklist';

export interface FileShareRoot {
  id: string;
  alias: string;
  path: string;
  enabled: boolean;
}

export interface FileSharePermissionSet {
  browse: boolean;
  download_file: boolean;
  download_archive: boolean;
  upload_file: boolean;
  upload_directory: boolean;
  create_directory: boolean;
  create_text: boolean;
  rename: boolean;
  delete: boolean;
  preview_image: boolean;
  search_current: boolean;
  search_global: boolean;
}

export interface FileShareUserRootPermissions {
  root_id: string;
  preset: FileSharePermissionPreset;
  permissions: FileSharePermissionSet;
}

export interface FileShareUserView {
  username: string;
  enabled: boolean;
  root_permissions: FileShareUserRootPermissions[];
  password_set: boolean;
  password_plain: string | null;
}

export interface FileShareUserSaveRequest {
  username: string;
  enabled: boolean;
  root_permissions: FileShareUserRootPermissions[];
  previous_username?: string | null;
  new_password?: string | null;
  clear_password: boolean;
}

export interface FileShareSettingsView {
  port: number;
  roots: FileShareRoot[];
  guest_access_enabled: boolean;
  guest_account: FileShareUserView;
  accounts: FileShareUserView[];
  session_ttl_minutes: number;
  ip_filter_mode: FileShareIpFilterMode;
  ip_rules: string[];
  image_preview_enabled: boolean;
  thumbnail_enabled: boolean;
  delete_mode: FileShareDeleteMode;
  remember_settings: boolean;
  auto_start_on_page_open: boolean;
  auto_start_with_windows: boolean;
}

export interface FileShareSettingsSaveRequest {
  port: number;
  roots: FileShareRoot[];
  guest_access_enabled: boolean;
  guest_account: FileShareUserSaveRequest;
  accounts: FileShareUserSaveRequest[];
  session_ttl_minutes: number;
  ip_filter_mode: FileShareIpFilterMode;
  ip_rules: string[];
  image_preview_enabled: boolean;
  thumbnail_enabled: boolean;
  delete_mode: FileShareDeleteMode;
  remember_settings: boolean;
  auto_start_on_page_open: boolean;
  auto_start_with_windows: boolean;
}

export interface FileShareConfig {
  port: number;
  shared_dirs: SharedDir[];
  password: string | null;
}

export interface FileShareStatus {
  is_active: boolean;
  connection_count: number;
  uptime_secs: number;
  server_url: string;
  all_urls: string[];
  shared_dirs: SharedDir[];
  connected_ips: string[];
}

export async function fileSharePickDirectory(): Promise<SharedDir | null> {
  return await invoke<SharedDir | null>('file_share_pick_directory');
}

export async function fileShareLoadSettings(): Promise<FileShareSettingsView> {
  return await invoke<FileShareSettingsView>('file_share_load_settings');
}

export async function fileShareSaveSettings(request: FileShareSettingsSaveRequest): Promise<FileShareSettingsView> {
  return await invoke<FileShareSettingsView>('file_share_save_settings', { request });
}

export async function fileShareStart(config: FileShareConfig): Promise<string> {
  return await invoke<string>('file_share_start', { config });
}

export async function fileShareStartSaved(): Promise<string> {
  return await invoke<string>('file_share_start_saved');
}

export async function fileShareStop(): Promise<void> {
  await invoke('file_share_stop');
}

export async function fileShareGetStatus(): Promise<FileShareStatus> {
  return await invoke<FileShareStatus>('file_share_get_status');
}

export interface AdminTaskStatus {
  installed: boolean;
  path_valid: boolean;
  last_error: string | null;
}

// ===== Updater =====

export const updaterApi = {
  getState: () => invoke<UpdateState>('get_update_state'),
  check: () => invoke<UpdateCheckResult>('check_update'),
  startDownload: () => invoke<void>('start_update_download'),
  cancelDownload: () => invoke<void>('cancel_update_download'),
  applyNow: () => invoke<void>('apply_update_now'),
  testServer: () => invoke<TestServerResult>('test_update_server'),
};

// ===== Error Code Lookup =====

export type ErrorCodeMode = 'single' | 'range' | 'keyword';

export interface ErrorCodeEntry {
  code: number;
  message_cn: string;
  message_en: string;
  solution: string;
  module: string;
  remark: string;
  source_file: string;
}

export interface ErrorCodeQueryRequest {
  mode: ErrorCodeMode;
  value: string;
  page: number;
}

export interface ErrorCodeQueryResult {
  entries: ErrorCodeEntry[];
  total: number;
  page: number;
  page_size: number;
}

export interface ErrorCodeSyncReport {
  file_count: number;
  row_count: number;
  last_synced_at: string;
}

export interface ErrorCodeMetaInfo {
  has_cache: boolean;
  last_synced_at: string | null;
  file_count: number;
  row_count: number;
}

export const errorCodeApi = {
  sync: () => invoke<ErrorCodeSyncReport>('error_code_sync'),
  query: (request: ErrorCodeQueryRequest) =>
    invoke<ErrorCodeQueryResult>('error_code_query', { request }),
  getMeta: () => invoke<ErrorCodeMetaInfo>('error_code_get_meta'),
};

export type ClipboardImportMode = 'replace' | 'merge';

export interface ClipboardImportReport {
  imported_items: number;
  imported_groups: number;
  backup_path: string | null;
}

export type NotepadArchitecture = 'x86' | 'x64' | 'arm64' | 'unknown';

export interface NotepadPluginStatus {
  installed: boolean;
  dll_path: string;
  config_path: string;
  config_exists: boolean;
}

export interface InstalledNotepadPlugin {
  name: string;
  dll_path: string;
}

export interface NotepadInstance {
  exe_path: string;
  install_dir: string;
  settings_dir: string;
  architecture: NotepadArchitecture;
  architecture_key: NotepadArchitecture;
  source: string;
  portable: boolean;
  running: boolean;
  requires_elevation: boolean;
  installed_plugins: InstalledNotepadPlugin[];
  enhance_any_lexer: NotepadPluginStatus;
}

export interface NotepadPluginPackage {
  url: string;
  sha256: string;
  size: number;
  install_dir: string;
  entry_dll: string;
}

export interface NotepadPluginRelease {
  version: string;
  notepad_compatible: string;
  packages: Partial<Record<NotepadArchitecture, NotepadPluginPackage>>;
}

export interface NotepadPluginCatalogEntry {
  id: string;
  name: string;
  publisher: string;
  description_zh: string;
  description_en: string;
  homepage: string;
  license: string;
  adapter: string;
  releases: NotepadPluginRelease[];
}

export interface NotepadPluginCatalog {
  schema_version: number;
  generated_at: string;
  plugins: NotepadPluginCatalogEntry[];
}

export interface NotepadPluginInstallProgress {
  plugin_id: string;
  phase: 'downloading' | 'verifying' | 'extracting' | 'installing' | 'complete';
}

export interface NotepadPluginInstallResult {
  target_path: string;
  restart_required: boolean;
  backup_path: string | null;
}

export interface EnhanceAnyLexerGlobal {
  indicator_id: number;
  offset: number;
  regex_error_style_id: number;
  regex_error_color: string;
}

export interface EnhanceAnyLexerRule {
  id: string;
  name: string;
  enabled: boolean;
  color: string;
  pattern: string;
  whitelist_styles: number[];
}

export interface EnhanceAnyLexerSection {
  lexer: string;
  excluded_styles: number[];
  rules: EnhanceAnyLexerRule[];
}

export interface EnhanceAnyLexerConfig {
  global: EnhanceAnyLexerGlobal;
  sections: EnhanceAnyLexerSection[];
}

export interface EnhanceAnyLexerSaveResult {
  config_path: string;
  backup_path: string | null;
  restart_required: boolean;
}

export const notepadExtensionsApi = {
  detectInstances: () =>
    invoke<NotepadInstance[]>('notepad_extensions_detect_instances'),
  validateInstance: (exePath: string) =>
    invoke<NotepadInstance>('notepad_extensions_validate_instance', { exePath }),
  pickExecutable: () => invoke<string | null>('notepad_extensions_pick_executable'),
  fetchCatalog: (serverUrl: string) =>
    invoke<NotepadPluginCatalog>('notepad_extensions_fetch_catalog', { serverUrl }),
  installPlugin: (
    serverUrl: string,
    exePath: string,
    pluginId: string,
    packageInfo: NotepadPluginPackage,
  ) =>
    invoke<NotepadPluginInstallResult>('notepad_extensions_install_plugin', {
      serverUrl,
      exePath,
      pluginId,
      package: packageInfo,
    }),
  readEnhanceConfig: (exePath: string) =>
    invoke<EnhanceAnyLexerConfig>('notepad_extensions_read_enhance_config', { exePath }),
  saveEnhanceConfig: (exePath: string, config: EnhanceAnyLexerConfig) =>
    invoke<EnhanceAnyLexerSaveResult>('notepad_extensions_save_enhance_config', {
      exePath,
      config,
    }),
};

export interface ImageCopyCrop {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ImageCopyResult {
  path: string;
  width: number;
  height: number;
  cropped: boolean;
}

// ─── Clipboard Manager ────────────────────────────────────

export const clipboardApi = {
  isEnabled: () => invoke<boolean>('cb_is_enabled'),
  enable: () => invoke<void>('cb_enable'),
  disable: () => invoke<void>('cb_disable'),
  list: (query: ClipboardListQuery) =>
    invoke<ClipboardListResult>('cb_list', { query }),
  get: (id: number) => invoke<ClipboardItem>('cb_get', { id }),
  delete: (id: number) => invoke<void>('cb_delete', { id }),
  deleteBatch: (ids: number[]) => invoke<void>('cb_delete_batch', { ids }),
  clear: (keepFavorites: boolean, groupId: number | null) =>
    invoke<number>('cb_clear', { keepFavorites, groupId }),
  clearAll: (keepFavorites: boolean) =>
    invoke<number>('cb_clear_all', { keepFavorites }),
  toggleFavorite: (id: number) =>
    invoke<ClipboardItem>('cb_toggle_favorite', { id }),
  togglePin: (id: number) => invoke<ClipboardItem>('cb_toggle_pin', { id }),
  listGroups: () => invoke<ClipboardGroup[]>('cb_groups_list'),
  createGroup: (name: string) => invoke<ClipboardGroup>('cb_groups_create', { name }),
  renameGroup: (id: number, name: string) =>
    invoke<ClipboardGroup>('cb_groups_rename', { id, name }),
  deleteGroup: (id: number) => invoke<void>('cb_groups_delete', { id }),
  moveToGroup: (itemId: number, groupId: number | null) =>
    invoke<ClipboardItem>('cb_move_to_group', { itemId, groupId }),
  setActiveGroup: (groupId: number | null) =>
    invoke<void>('cb_set_active_group', { groupId }),
  pickImageFile: () => invoke<string | null>('cb_pick_image_file'),
  copyImageFile: (path: string, crop?: ImageCopyCrop | null) =>
    invoke<ImageCopyResult>('cb_copy_image_file', { path, crop: crop ?? null }),
  isExplorerContextMenuRegistered: () =>
    invoke<boolean>('cb_is_explorer_context_menu_registered'),
  reorderFavorites: (ids: number[]) => invoke<void>('cb_reorder_favorites', { ids }),
  paste: (id: number) => invoke<void>('cb_paste', { id }),
  pastePlain: (id: number) => invoke<void>('cb_paste_plain', { id }),
  copy: (id: number) => invoke<void>('cb_copy', { id }),
  pasteAsFiles: (id: number) => invoke<void>('cb_paste_as_files', { id }),
  pasteAsPath: (id: number) => invoke<void>('cb_paste_as_path', { id }),
  checkFilePaths: (ids: number[]) =>
    invoke<FilePathStatus[]>('cb_check_file_paths', { ids }),
  saveImageAs: (id: number, targetPath: string) =>
    invoke<void>('cb_save_image_as', { id, targetPath }),
  openInExplorer: (path: string) => invoke<void>('cb_open_in_explorer', { path }),
  mergePaste: (ids: number[], separator?: string | null) =>
    invoke<void>('cb_merge_paste', { ids, separator: separator ?? null }),
  showImagePreview: (id: number, token: number) =>
    invoke<void>('cb_show_image_preview', { id, token }),
  showTextPreview: (id: number, token: number) =>
    invoke<void>('cb_show_text_preview', { id, token }),
  getImagePreviewPayload: () =>
    invoke<ClipboardImagePreviewPayload | null>('cb_get_image_preview_payload'),
  getTextPreviewPayload: () =>
    invoke<ClipboardTextPreviewPayload | null>('cb_get_text_preview_payload'),
  hidePreview: (token?: number | null) =>
    invoke<void>('cb_hide_preview', { token: token ?? null }),
  togglePreviewFullscreen: (label: string) =>
    invoke<boolean>('cb_toggle_preview_fullscreen', { label }),
  debugWindowSnapshot: (context: string) =>
    invoke<void>('cb_debug_window_snapshot', { context }),
  togglePanel: () => invoke<void>('cb_toggle_panel'),
  stats: () => invoke<ClipboardStats>('cb_stats'),
  getSettings: () => invoke<ClipboardSettings>('cb_get_settings'),
  saveSettings: (settings: ClipboardSettings) =>
    invoke<ClipboardSettings>('cb_save_settings', { settings }),
  exportData: (path: string, includeImages: boolean) =>
    invoke<void>('cb_export', { path, includeImages }),
  importData: (path: string, mode: ClipboardImportMode) =>
    invoke<ClipboardImportReport>('cb_import', { path, mode }),
  dbOptimize: () => invoke<void>('cb_db_optimize'),
  dbVacuum: () => invoke<void>('cb_db_vacuum'),
  resetConfig: () => invoke<ClipboardSettings>('cb_reset_config'),
  resetAll: () => invoke<ClipboardSettings>('cb_reset_all'),
  setHotkey: (hotkey: string) => invoke<void>('cb_set_hotkey', { hotkey }),
  enableWinV: () => invoke<void>('cb_enable_win_v'),
  disableWinV: () => invoke<void>('cb_disable_win_v'),
  isWinVEnabled: () => invoke<boolean>('cb_is_win_v_enabled'),
  isElevated: () => invoke<boolean>('cb_is_elevated'),
  isRunAsAdminEnabled: () => invoke<boolean>('cb_is_run_as_admin_enabled'),
  adminTaskStatus: () => invoke<AdminTaskStatus>('cb_admin_task_status'),
  adminTaskCreate: () => invoke<AdminTaskStatus>('cb_admin_task_create'),
  adminTaskRemove: () => invoke<AdminTaskStatus>('cb_admin_task_remove'),
  setRunAsAdmin: (enable: boolean) =>
    invoke<AdminTaskStatus>('cb_set_run_as_admin', { enable }),
  setPanelPinned: (pinned: boolean) =>
    invoke<void>('cb_set_panel_pinned', { pinned }),
  isPanelPinned: () => invoke<boolean>('cb_is_panel_pinned'),
  openSettings: () => invoke<void>('cb_open_settings'),
};
