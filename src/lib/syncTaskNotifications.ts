import type {
  CopyState,
  DeployState,
  LocalExecState,
  TaskGroupListItem,
  TaskSummaryStatus,
} from './tauri.ts';

export type SyncTaskNotificationKind =
  | 'queued'
  | 'copy_started'
  | 'copy_completed'
  | 'copy_failed'
  | 'local_exec_started'
  | 'local_exec_completed'
  | 'local_exec_failed'
  | 'deploy_started'
  | 'deploy_completed'
  | 'deploy_failed'
  | 'task_paused'
  | 'task_resumed'
  | 'task_cancelled'
  | 'task_interrupted';

export interface SyncTaskNotificationEvent {
  kind: SyncTaskNotificationKind;
  taskName: string;
}

export interface SyncTaskNotificationDispatcherOptions {
  isEnabled: () => boolean;
  render: (event: SyncTaskNotificationEvent) => { title: string; body: string };
  show: (title: string, body: string) => Promise<void>;
  onError: (error: unknown, event: SyncTaskNotificationEvent) => void;
}

interface TrackedTaskRun {
  runId: string;
  copyStatus: CopyState;
  localExecStatus: LocalExecState;
  deployStatus: DeployState;
  summaryStatus: TaskSummaryStatus;
  notify: boolean;
}

function taskName(group: TaskGroupListItem): string {
  return group.display_name.trim() || group.folder_name.trim();
}

function isDeployFailure(status: DeployState): boolean {
  return status === 'failed' || status === 'partial_failed';
}

function isLocalExecFailure(status: LocalExecState): boolean {
  return status === 'failed' || status === 'partial_failed';
}

function isActiveAfterPause(status: TaskSummaryStatus): boolean {
  return status === 'copying' || status === 'local_executing' || status === 'deploying';
}

export function createSyncTaskNotificationDispatcher(
  options: SyncTaskNotificationDispatcherOptions,
) {
  let deliveryQueue = Promise.resolve();

  async function deliver(event: SyncTaskNotificationEvent): Promise<void> {
    if (!options.isEnabled()) return;

    try {
      const message = options.render(event);
      await options.show(message.title, message.body);
    } catch (error) {
      options.onError(error, event);
    }
  }

  function enqueue(event: SyncTaskNotificationEvent): Promise<void> {
    // Preserve every milestone when several task phases change in one snapshot.
    deliveryQueue = deliveryQueue.then(
      () => deliver(event),
      () => deliver(event),
    );
    return deliveryQueue;
  }

  return { enqueue };
}

export function createSyncTaskNotificationTracker(initialGroups: TaskGroupListItem[] = []) {
  const trackedRuns = new Map<string, TrackedTaskRun>();

  function remember(groups: TaskGroupListItem[]) {
    for (const group of groups) {
      if (group.task_config_id === null || group.latest_run_id === null) continue;
      trackedRuns.set(group.task_group_id, {
        runId: group.latest_run_id,
        copyStatus: group.copy_status,
        localExecStatus: group.local_exec_status,
        deployStatus: group.deploy_status,
        summaryStatus: group.summary_status,
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
        if (previous.summaryStatus !== 'paused' && group.summary_status === 'paused') {
          notifications.push({ kind: 'task_paused', taskName: name });
        } else if (previous.summaryStatus === 'paused' && isActiveAfterPause(group.summary_status)) {
          notifications.push({ kind: 'task_resumed', taskName: name });
        }

        if (previous.copyStatus !== 'running' && group.copy_status === 'running') {
          notifications.push({ kind: 'copy_started', taskName: name });
        }
        if (previous.copyStatus !== 'completed' && group.copy_status === 'completed') {
          notifications.push({ kind: 'copy_completed', taskName: name });
        }
        if (
          previous.copyStatus !== group.copy_status
          && group.copy_status === 'failed'
        ) {
          notifications.push({ kind: 'copy_failed', taskName: name });
        }
        if (previous.localExecStatus !== 'running' && group.local_exec_status === 'running') {
          notifications.push({ kind: 'local_exec_started', taskName: name });
        }
        if (previous.localExecStatus !== 'completed' && group.local_exec_status === 'completed') {
          notifications.push({ kind: 'local_exec_completed', taskName: name });
        }
        if (
          previous.localExecStatus !== group.local_exec_status
          && isLocalExecFailure(group.local_exec_status)
        ) {
          notifications.push({ kind: 'local_exec_failed', taskName: name });
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
        if (previous.summaryStatus !== 'cancelled' && group.summary_status === 'cancelled') {
          notifications.push({ kind: 'task_cancelled', taskName: name });
        }
        if (previous.summaryStatus !== 'interrupted' && group.summary_status === 'interrupted') {
          notifications.push({ kind: 'task_interrupted', taskName: name });
        }
      } else if (!isSameRun && shouldNotify) {
        // A normal scanned copy first arrives as pending, then produces transitions above.
        // If the pending snapshot was missed, still report the active phase without
        // inventing copy events for deploy-only retry runs.
        if (group.copy_status === 'running') {
          notifications.push({ kind: 'copy_started', taskName: name });
        } else if (group.local_exec_status === 'running') {
          notifications.push({ kind: 'local_exec_started', taskName: name });
        } else if (group.deploy_status === 'running') {
          notifications.push({ kind: 'deploy_started', taskName: name });
        }
      }

      trackedRuns.set(group.task_group_id, {
        runId: group.latest_run_id,
        copyStatus: group.copy_status,
        localExecStatus: group.local_exec_status,
        deployStatus: group.deploy_status,
        summaryStatus: group.summary_status,
        notify: shouldNotify,
      });
      queuedRunIds.delete(group.latest_run_id);
    }

    return notifications;
  }

  remember(initialGroups);
  return { collect, remember, markQueued };
}
