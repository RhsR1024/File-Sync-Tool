import { shallowReactive } from 'vue';

import type {
  StartManualCopyTaskRequest,
  StartManualDeployTaskRequest,
  TaskGroup,
  TaskGroupListItem,
  TaskLogEntry,
  TaskRunHandle,
} from './tauri.ts';
import {
  cancelTaskRun,
  clearTaskGroup,
  clearTaskGroups,
  getTaskGroupDetail,
  listTaskGroups,
  pauseTaskRun,
  resumeTaskRun,
  retryTaskGroupDeploy,
  startManualCopyTask,
  startManualDeployTask,
} from './tauri.ts';

export interface TaskStateStoreApi {
  listTaskGroups: () => Promise<TaskGroupListItem[]>;
  getTaskGroupDetail: (taskGroupId: string) => Promise<TaskGroup>;
  startManualCopyTask: (request: StartManualCopyTaskRequest) => Promise<TaskRunHandle>;
  startManualDeployTask: (request: StartManualDeployTaskRequest) => Promise<TaskRunHandle>;
  clearTaskGroup: (taskGroupId: string) => Promise<void>;
  clearTaskGroups: () => Promise<void>;
  cancelTaskRun: (taskGroupId: string, runId: string) => Promise<void>;
  pauseTaskRun: (taskGroupId: string, runId: string) => Promise<void>;
  resumeTaskRun: (taskGroupId: string, runId: string) => Promise<void>;
  retryTaskGroupDeploy: (taskGroupId: string) => Promise<TaskRunHandle>;
}

const defaultApi: TaskStateStoreApi = {
  listTaskGroups,
  getTaskGroupDetail,
  startManualCopyTask,
  startManualDeployTask,
  clearTaskGroup,
  clearTaskGroups,
  cancelTaskRun,
  pauseTaskRun,
  resumeTaskRun,
  retryTaskGroupDeploy,
};

const MAX_TASK_LOG_ENTRIES = 10_000;
const MAX_GROUP_LOG_ENTRIES = 2_000;
const MAX_CACHED_DETAILS = 32;

export interface ManualDeploySession {
  task_group_id: string;
  run_id: string;
  display_name: string;
  server_ids: string[];
  started_at: string;
}

export function createTaskStateStore(apiOverrides: Partial<TaskStateStoreApi> = {}) {
  const api = { ...defaultApi, ...apiOverrides };
  const state = shallowReactive({
    groups: [] as TaskGroupListItem[],
    selectedTaskGroupId: null as string | null,
    selectedGroupDetail: null as TaskGroup | null,
    groupDetails: shallowReactive({} as Record<string, TaskGroup>),
    isHydrated: false,
    isLoadingDetail: false,
    taskLogs: shallowReactive([] as TaskLogEntry[]),
    taskLogsByGroup: shallowReactive({} as Record<string, TaskLogEntry[]>),
    latestManualDeploy: null as ManualDeploySession | null,
  });

  let groupsVersion = 0;
  let hydration: Promise<void> | null = null;
  let selectionVersion = 0;

  function replaceGroups(groups: TaskGroupListItem[]) {
    ++groupsVersion;
    state.groups = groups;
    state.isHydrated = true;
    const ids = new Set(groups.map(group => group.task_group_id));
    for (const id of Object.keys(state.groupDetails)) {
      if (!ids.has(id)) delete state.groupDetails[id];
    }
    for (const id of Object.keys(state.taskLogsByGroup)) {
      if (!ids.has(id)) delete state.taskLogsByGroup[id];
    }
    const retainedLogs = state.taskLogs.filter(entry => !entry.task_group_id || ids.has(entry.task_group_id));
    if (retainedLogs.length !== state.taskLogs.length) {
      state.taskLogs.splice(0, state.taskLogs.length, ...retainedLogs);
    }
    if (state.selectedTaskGroupId && !ids.has(state.selectedTaskGroupId)) {
      ++selectionVersion;
      state.selectedTaskGroupId = null;
      state.selectedGroupDetail = null;
      state.isLoadingDetail = false;
    }
  }

  function hydrateTaskState(): Promise<void> {
    if (hydration) return hydration;
    const version = groupsVersion;
    hydration = Promise.resolve().then(async () => {
      const groups = await api.listTaskGroups();
      // Live events received during the request are newer than its response.
      if (version === groupsVersion) replaceGroups(groups);
    }).finally(() => { hydration = null; });
    return hydration;
  }

  function cacheDetail(taskGroupId: string, detail: TaskGroup) {
    delete state.groupDetails[taskGroupId];
    state.groupDetails[taskGroupId] = detail;
    const ids = Object.keys(state.groupDetails);
    for (const id of ids) {
      if (Object.keys(state.groupDetails).length <= MAX_CACHED_DETAILS) break;
      if (id !== state.selectedTaskGroupId && id !== state.latestManualDeploy?.task_group_id) {
        delete state.groupDetails[id];
      }
    }
  }

  async function selectTaskGroup(taskGroupId: string) {
    const version = ++selectionVersion;
    state.selectedTaskGroupId = taskGroupId;
    state.selectedGroupDetail = state.groupDetails[taskGroupId] ?? null;
    const previousDetail = state.groupDetails[taskGroupId];
    state.isLoadingDetail = true;
    try {
      const detail = await api.getTaskGroupDetail(taskGroupId);
      if (version !== selectionVersion) return;
      // A detail event may already have advanced this task while IPC was pending.
      if (state.groupDetails[taskGroupId] === previousDetail) {
        cacheDetail(taskGroupId, detail);
        state.selectedGroupDetail = detail;
      }
    } finally {
      if (version === selectionVersion) state.isLoadingDetail = false;
    }
  }

  let latestRevision = 0;

  function applyGroupsSnapshot(payload: { revision?: number; groups: TaskGroupListItem[] }) {
    if (payload.revision !== undefined && payload.revision <= latestRevision) return false;
    if (payload.revision !== undefined) latestRevision = payload.revision;
    replaceGroups(payload.groups);
    return true;
  }

  function applyDetailSnapshot(payload: { task_group_id: string; group: TaskGroup }) {
    cacheDetail(payload.task_group_id, payload.group);
    if (payload.task_group_id === state.selectedTaskGroupId) {
      state.selectedGroupDetail = payload.group;
    }
  }

  function appendTaskLog(entry: TaskLogEntry) {
    state.taskLogs.push(entry);
    if (entry.task_group_id) {
      const groupLogs = state.taskLogsByGroup[entry.task_group_id]
        ?? (state.taskLogsByGroup[entry.task_group_id] = shallowReactive([] as TaskLogEntry[]));
      groupLogs.push(entry);
      if (groupLogs.length > MAX_GROUP_LOG_ENTRIES) {
        groupLogs.splice(0, MAX_GROUP_LOG_ENTRIES / 10);
      }
    }
    // Per-group indexes must share the global retention budget. A per-group
    // limit alone leaves one permanent array for every task ever observed.
    if (state.taskLogs.length > MAX_TASK_LOG_ENTRIES) {
      // Trim in batches: shifting a reactive 10,000-entry array on every log
      // would perform thousands of proxy writes per incoming event.
      const removed = new Set(state.taskLogs.splice(0, MAX_TASK_LOG_ENTRIES / 10));
      const affectedGroups = new Set([...removed].map(log => log.task_group_id));
      for (const id of affectedGroups) {
        if (!id) continue;
        const groupLogs = state.taskLogsByGroup[id];
        if (!groupLogs) continue;
        let count = 0;
        while (count < groupLogs.length && removed.has(groupLogs[count])) count++;
        if (count === groupLogs.length) delete state.taskLogsByGroup[id];
        else if (count) groupLogs.splice(0, count);
      }
    }
  }

  async function startManualCopy(request: StartManualCopyTaskRequest) {
    const handle = await api.startManualCopyTask(request);
    await hydrateTaskState();
    return handle;
  }

  async function startManualDeploy(request: StartManualDeployTaskRequest) {
    const handle = await api.startManualDeployTask(request);
    state.latestManualDeploy = {
      task_group_id: handle.task_group_id,
      run_id: handle.run_id,
      display_name: request.display_name?.trim() || request.folder_name?.trim() || 'manual-deploy',
      server_ids: [...new Set(request.bindings.map(binding => binding.server_id))],
      started_at: new Date().toISOString(),
    };
    await hydrateTaskState();
    try {
      const previousDetail = state.groupDetails[handle.task_group_id];
      const detail = await api.getTaskGroupDetail(handle.task_group_id);
      if (state.groupDetails[handle.task_group_id] === previousDetail) {
        cacheDetail(handle.task_group_id, detail);
      }
    } catch {
      // The global detail-snapshot listener will populate this as the run advances.
    }
    return handle;
  }

  return Object.assign(state, {
    hydrateTaskState,
    selectTaskGroup,
    applyGroupsSnapshot,
    applyDetailSnapshot,
    appendTaskLog,
    startManualCopy,
    startManualDeploy,
  });
}

export const taskStateStore = createTaskStateStore();
