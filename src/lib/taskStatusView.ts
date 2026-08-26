import type { TaskGroupListItem, TaskGroup, ServerRollup } from './tauri';

export function buildTaskRows(groups: TaskGroupListItem[]): TaskGroupListItem[] {
  return [...groups].sort((a, b) => b.started_at.localeCompare(a.started_at));
}

export interface ServerFailure {
  serverId: string;
  serverLabel: string;
  message: string;
}

export interface TaskDetailSections {
  serverFailures: ServerFailure[];
  runs: TaskGroup['runs'];
}

export function serverDisplayLabel(server: Pick<ServerRollup, 'server_id' | 'server_name' | 'server_host'>): string {
  const name = server.server_name.trim();
  const host = server.server_host.trim();

  if (name && host && name !== host) {
    return `${name} (${host})`;
  }
  return name || host || server.server_id;
}

export function buildTaskDetailSections(group: TaskGroup): TaskDetailSections {
  return {
    serverFailures: group.server_rollups
      .filter((rollup: ServerRollup) => rollup.last_error_message)
      .map((rollup: ServerRollup) => ({
        serverId: rollup.server_id,
        serverLabel: serverDisplayLabel(rollup),
        message: rollup.last_error_message!,
      })),
    runs: group.runs,
  };
}
