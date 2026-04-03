import type { TaskGroupListItem, TaskGroup, ServerRollup } from './tauri';

export function buildTaskRows(groups: TaskGroupListItem[]): TaskGroupListItem[] {
  return [...groups].sort((a, b) => b.started_at.localeCompare(a.started_at));
}

export interface ServerFailure {
  serverId: string;
  serverName: string;
  message: string;
}

export interface TaskDetailSections {
  serverFailures: ServerFailure[];
  runs: TaskGroup['runs'];
}

export function buildTaskDetailSections(group: TaskGroup): TaskDetailSections {
  return {
    serverFailures: group.server_rollups
      .filter((rollup: ServerRollup) => rollup.last_error_message)
      .map((rollup: ServerRollup) => ({
        serverId: rollup.server_id,
        serverName: rollup.server_name,
        message: rollup.last_error_message!,
      })),
    runs: group.runs,
  };
}
