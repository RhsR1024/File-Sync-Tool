use crate::task_domain::{TaskGroup, TaskSummaryStatus};
use crate::task_manager::TaskManager;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRetentionCandidate {
    pub task_group_id: String,
    pub display_name: String,
    pub local_path: String,
    pub finished_at: String,
    pub bytes: u64,
    pub package_exists: bool,
    pub eligible: bool,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRetentionPreview {
    pub days: u32,
    pub cutoff: String,
    pub candidates: Vec<SyncRetentionCandidate>,
    pub eligible_packages: usize,
    pub eligible_records: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRetentionResult {
    pub removed_packages: usize,
    pub removed_records: usize,
    pub freed_bytes: u64,
    pub skipped: usize,
    pub errors: Vec<String>,
}

fn is_terminal(status: &TaskSummaryStatus) -> bool {
    matches!(
        status,
        TaskSummaryStatus::Completed
            | TaskSummaryStatus::PartialFailed
            | TaskSummaryStatus::Failed
            | TaskSummaryStatus::Cancelled
            | TaskSummaryStatus::Interrupted
    )
}

fn timestamp(group: &TaskGroup) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(group.finished_at.as_deref().unwrap_or(&group.started_at)).ok()
}

fn module_key(group: &TaskGroup) -> String {
    if let Some(module_id) = group.artifact.module_id.as_deref() {
        return module_id.to_string();
    }
    let parts = group
        .source_path
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let version_index = parts
        .iter()
        .position(|part| part.starts_with('B') && part.contains('.'));
    let identity = version_index
        .and_then(|index| {
            Some(format!(
                "{}|{}",
                parts.get(index.wrapping_sub(1))?,
                parts.get(index + 1).unwrap_or(&"")
            ))
        })
        .unwrap_or_else(|| group.source_path.clone());
    format!(
        "{}|{identity}",
        group.task_config_id.as_deref().unwrap_or("manual")
    )
}

fn is_retryable(group: &TaskGroup) -> bool {
    group.had_failures
        || matches!(
            group.summary_status,
            TaskSummaryStatus::Cancelled | TaskSummaryStatus::Interrupted
        )
}

fn path_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| path_size(&entry.path()))
        .sum()
}

fn safe_existing_target(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("本地包根目录不可访问: {error}"))?;
    let target = target
        .canonicalize()
        .map_err(|error| format!("目标路径不可访问: {error}"))?;
    if target == root || !target.starts_with(&root) {
        return Err("目标路径不在本地包根目录内".to_string());
    }
    if fs::symlink_metadata(&target)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(true)
    {
        return Err("目标路径是符号链接".to_string());
    }
    Ok(target)
}

fn build_preview(
    root: &Path,
    groups: &[TaskGroup],
    days: u32,
) -> Result<SyncRetentionPreview, String> {
    let days = days.clamp(1, 365);
    let cutoff = Utc::now() - Duration::days(i64::from(days));
    let mut latest_success = HashMap::<String, DateTime<chrono::FixedOffset>>::new();
    for group in groups
        .iter()
        .filter(|group| group.summary_status == TaskSummaryStatus::Completed)
    {
        if let Some(time) = timestamp(group) {
            latest_success
                .entry(module_key(group))
                .and_modify(|current| *current = (*current).max(time))
                .or_insert(time);
        }
    }

    let protected_paths = groups
        .iter()
        .filter(|group| {
            !is_terminal(&group.summary_status)
                || is_retryable(group)
                || (group.summary_status == TaskSummaryStatus::Completed
                    && timestamp(group)
                        .is_some_and(|time| latest_success.get(&module_key(group)) == Some(&time)))
        })
        .map(|group| group.local_target_path.clone())
        .collect::<HashSet<_>>();

    let mut candidates = Vec::new();
    for group in groups {
        let Some(time) = timestamp(group) else {
            continue;
        };
        if time.with_timezone(&Utc) >= cutoff {
            continue;
        }
        let path = Path::new(&group.local_target_path);
        let exists = path.exists();
        let mut skip_reason = None;
        if !is_terminal(&group.summary_status) || is_retryable(group) {
            skip_reason = Some("任务仍在活动、等待或可恢复状态".to_string());
        } else if protected_paths.contains(&group.local_target_path) {
            skip_reason = Some("保留该模块最近一次成功包".to_string());
        } else if exists {
            if let Err(error) = safe_existing_target(root, path) {
                skip_reason = Some(error);
            }
        }
        let bytes = if exists && skip_reason.is_none() {
            path_size(path)
        } else {
            0
        };
        candidates.push(SyncRetentionCandidate {
            task_group_id: group.task_group_id.clone(),
            display_name: group.display_name.clone(),
            local_path: group.local_target_path.clone(),
            finished_at: time.to_rfc3339(),
            bytes,
            package_exists: exists,
            eligible: skip_reason.is_none(),
            skip_reason,
        });
    }
    let eligible = candidates.iter().filter(|candidate| candidate.eligible);
    Ok(SyncRetentionPreview {
        days,
        cutoff: cutoff.to_rfc3339(),
        eligible_packages: eligible
            .clone()
            .filter(|candidate| candidate.package_exists)
            .count(),
        eligible_records: eligible.clone().count(),
        total_bytes: eligible.map(|candidate| candidate.bytes).sum(),
        candidates,
    })
}

#[tauri::command]
pub async fn preview_sync_retention(
    days: u32,
    state: tauri::State<'_, crate::AppState>,
) -> Result<SyncRetentionPreview, String> {
    let root = PathBuf::from(state.config.lock().unwrap().local_path.clone());
    let groups = state.task_manager.snapshot_state().groups;
    tauri::async_runtime::spawn_blocking(move || build_preview(&root, &groups, days))
        .await
        .map_err(|error| format!("清理预览任务失败: {error}"))?
}

#[tauri::command]
pub async fn apply_sync_retention(
    days: u32,
    state: tauri::State<'_, crate::AppState>,
) -> Result<SyncRetentionResult, String> {
    let root = PathBuf::from(state.config.lock().unwrap().local_path.clone());
    let manager: TaskManager = state.task_manager.clone();
    let groups = manager.snapshot_state().groups;
    tauri::async_runtime::spawn_blocking(move || apply_retention(&root, &groups, &manager, days))
        .await
        .map_err(|error| format!("清理任务失败: {error}"))?
}

fn apply_retention(
    root: &Path,
    groups: &[TaskGroup],
    manager: &TaskManager,
    days: u32,
) -> Result<SyncRetentionResult, String> {
    let preview = build_preview(root, groups, days)?;
    let mut result = SyncRetentionResult {
        removed_packages: 0,
        removed_records: 0,
        freed_bytes: 0,
        skipped: preview
            .candidates
            .iter()
            .filter(|candidate| !candidate.eligible)
            .count(),
        errors: vec![],
    };
    for candidate in preview
        .candidates
        .into_iter()
        .filter(|candidate| candidate.eligible)
    {
        if candidate.package_exists {
            let target = match safe_existing_target(root, Path::new(&candidate.local_path)) {
                Ok(path) => path,
                Err(error) => {
                    result
                        .errors
                        .push(format!("{}: {error}", candidate.local_path));
                    continue;
                }
            };
            let remove_result = if target.is_dir() {
                fs::remove_dir_all(&target)
            } else {
                fs::remove_file(&target)
            };
            if let Err(error) = remove_result {
                result.errors.push(format!("{}: {error}", target.display()));
                continue;
            }
            result.removed_packages += 1;
            result.freed_bytes += candidate.bytes;
        }
        match manager.clear_task_group(&candidate.task_group_id) {
            Ok(()) => result.removed_records += 1,
            Err(error) => result.errors.push(error),
        }
    }
    Ok(result)
}

pub fn start_automatic_sync_retention(
    config: Arc<Mutex<crate::config::AppConfig>>,
    manager: TaskManager,
) {
    tauri::async_runtime::spawn(async move {
        let mut last_run_date = None;
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            let today = chrono::Local::now().date_naive();
            let settings = {
                let config = config.lock().unwrap();
                (
                    config.sync_retention_enabled,
                    config.sync_retention_days,
                    PathBuf::from(&config.local_path),
                    config.max_task_records,
                )
            };
            if settings.0 && last_run_date != Some(today) {
                let groups = manager.snapshot_state().groups;
                let manager_clone = manager.clone();
                let root = settings.2;
                match tauri::async_runtime::spawn_blocking(move || {
                    apply_retention(&root, &groups, &manager_clone, settings.1)
                })
                .await
                {
                    Ok(Ok(result)) => {
                        log::info!(
                            "[sync-retention] removed packages={}, records={}, errors={}",
                            result.removed_packages,
                            result.removed_records,
                            result.errors.len()
                        );
                        last_run_date = Some(today);
                    }
                    Ok(Err(error)) => {
                        log::warn!("[sync-retention] automatic cleanup failed: {error}")
                    }
                    Err(error) => {
                        log::warn!("[sync-retention] automatic cleanup task failed: {error}")
                    }
                }
            }
            manager.prune_terminal_groups(settings.3 as usize);
            tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_and_outside_targets_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(safe_existing_target(root.path(), root.path()).is_err());
        assert!(safe_existing_target(root.path(), outside.path()).is_err());
    }
}
