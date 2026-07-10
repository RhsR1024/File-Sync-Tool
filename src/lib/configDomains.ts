import type { AppConfig, AppDomainConfigPatch, SyncConfigPatch } from './tauri';

export function buildSyncPatch(config: AppConfig): SyncConfigPatch {
  return {
    tasks: config.tasks,
    local_path: config.local_path,
    interval_minutes: config.interval_minutes,
    time_ranges: config.time_ranges,
    file_extensions: config.file_extensions,
    filename_includes: config.filename_includes,
    deploy_enabled: config.deploy_enabled,
    servers: config.servers,
    command_groups: config.command_groups,
    local_command_groups: config.local_command_groups,
    stability_check_secs: config.stability_check_secs,
    recent_file_guard_mins: config.recent_file_guard_mins,
    copy_buffer_size_kb: config.copy_buffer_size_kb,
  };
}

export function buildAppPatch(config: AppConfig): AppDomainConfigPatch {
  return {
    launch_and_auto_scan: config.launch_and_auto_scan,
    launch_and_auto_start_file_share: config.launch_and_auto_start_file_share,
    close_to_tray: config.close_to_tray,
    max_log_lines: config.max_log_lines,
    max_task_records: config.max_task_records,
    appliance_ssh_api_timeout_secs: config.appliance_ssh_api_timeout_secs,
    framework_password_api_timeout_secs: config.framework_password_api_timeout_secs,
    disk_cleanup_http_timeout_secs: config.disk_cleanup_http_timeout_secs,
    disk_cleanup_linux_mode: config.disk_cleanup_linux_mode,
    update_server_url: config.update_server_url,
    notify_on_new_version: config.notify_on_new_version,
    clipboard: config.clipboard,
  };
}
