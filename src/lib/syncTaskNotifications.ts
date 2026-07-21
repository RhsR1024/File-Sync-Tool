import type { CopyState, DeployState, TaskGroupListItem } from './tauri.ts';

export type SyncTaskNotificationKind =
  | 'queued'
  | 'copy_started'
  | 'copy_completed'
  | 'copy_failed'
  | 'deploy_started'
  | 'deploy_completed'
  | 'deploy_failed';

export interface SyncTaskNotificationEvent {
  kind: SyncTaskNotificationKind;
  taskName: string;
}

interface TrackedTaskRun {
  runId: string;
  copyStatus: CopyState;
  deployStatus: DeployState;
  notify: boolean;
}

function taskName(group: TaskGroupListItem): string {
  return group.display_name.trim() || group.folder_name.trim();
}

function isDeployFailure(status: DeployState): boolean {
  return status === 'failed' || status === 'partial_failed' || status === 'interrupted';
}

export function createSyncTaskNotificationTracker(initialGroups: TaskGroupListItem[] = []) {
  const trackedRuns = new Map<string, TrackedTaskRun>();

  function remember(groups: TaskGroupListItem[]) {
    for (const group of groups) {
      if (group.task_config_id === null || group.latest_run_id === null) continue;
      trackedRuns.set(group.task_group_id, {
        runId: group.latest_run_id,
        copyStatus: group.copy_status,
        deployStatus: group.deploy_status,
        notify: true,
      });
    }
  }

  const queuedRunIds = new Set<string>();

  function markQueued(runId: string) {
    let matchedTrackedRun = false;
    for (const tracked of trackedRuns.values()) {
      if (tracked.runId === runId) {
        tracked.notify = true;
        matchedTrackedRun = true;
      }
    }
    if (!matchedTrackedRun) queuedRunIds.add(runId);
  }

  function collect(groups: TaskGroupListItem[]): SyncTaskNotificationEvent[] {
    const notifications: SyncTaskNotificationEvent[] = [];

    for (const group of groups) {
      if (group.task_config_id === null || group.latest_run_id === null) continue;

      const previous = trackedRuns.get(group.task_group_id);
      const isSameRun = previous?.runId === group.latest_run_id;
      const name = taskName(group);
      const shouldNotify = isSameRun
        ? previous?.notify === true
        : queuedRunIds.has(group.latest_run_id);

      if (isSameRun && previous && shouldNotify) {
        if (previous.copyStatus !== 'running' && group.copy_status === 'running') {
          notifications.push({ kind: 'copy_started', taskName: name });
        }
        if (previous.copyStatus !== 'completed' && group.copy_status === 'completed') {
          notifications.push({ kind: 'copy_completed', taskName: name });
        }
        if (
          previous.copyStatus !== group.copy_status
          && (group.copy_status === 'failed' || group.copy_status === 'interrupted')
        ) {
          notifications.push({ kind: 'copy_failed', taskName: name });
        }
        if (previous.deployStatus !== 'running' && group.deploy_status === 'running') {
          notifications.push({ kind: 'deploy_started', taskName: name });
        }
        if (previous.deployStatus !== 'completed' && group.deploy_status === 'completed') {
          notifications.push({ kind: 'deploy_completed', taskName: name });
        }
        if (
          previous.deployStatus !== group.deploy_status
          && isDeployFailure(group.deploy_status)
        ) {
          notifications.push({ kind: 'deploy_failed', taskName: name });
        }
      } else if (!isSameRun && shouldNotify) {
        // A normal scanned copy first arrives as pending, then produces transitions above.
        // If the pending snapshot was missed, still report the active phase without
        // inventing copy events for deploy-only retry runs.
        if (group.copy_status === 'running') {
          notifications.push({ kind: 'copy_started', taskName: name });
        } else if (group.deploy_status === 'running') {
          notifications.push({ kind: 'deploy_started', taskName: name });
        }
      }

      trackedRuns.set(group.task_group_id, {
        runId: group.latest_run_id,
        copyStatus: group.copy_status,
        deployStatus: group.deploy_status,
        notify: shouldNotify,
      });
      queuedRunIds.delete(group.latest_run_id);
    }

    return notifications;
  }

  remember(initialGroups);
  return { collect, remember, markQueued };
}
