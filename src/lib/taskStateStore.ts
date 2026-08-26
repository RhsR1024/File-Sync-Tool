import { reactive } from 'vue';

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

export interface ManualDeploySession {
  task_group_id: string;
  run_id: string;
  display_name: string;
  server_ids: string[];
  started_at: string;
}

export function createTaskStateStore(apiOverrides: Partial<TaskStateStoreApi> = {}) {
  const api = { ...defaultApi, ...apiOverrides };
  const state = reactive({
    groups: [] as TaskGroupListItem[],
    selectedTaskGroupId: null as string | null,
    selectedGroupDetail: null as TaskGroup | null,
    groupDetails: {} as Record<string, TaskGroup>,
    isHydrated: false,
    isLoadingDetail: false,
    taskLogs: [] as TaskLogEntry[],
    latestManualDeploy: null as ManualDeploySession | null,
  });

  async function hydrateTaskState() {
    state.groups = await api.listTaskGroups();
    state.isHydrated = true;
  }

  async function selectTaskGroup(taskGroupId: string) {
    state.selectedTaskGroupId = taskGroupId;
    state.isLoadingDetail = true;
    try {
      const detail = await api.getTaskGroupDetail(taskGroupId);
      state.selectedGroupDetail = detail;
      state.groupDetails[taskGroupId] = detail;
    } finally {
      state.isLoadingDetail = false;
    }
  }

  function applyGroupsSnapshot(payload: { groups: TaskGroupListItem[] }) {
    state.groups = payload.groups;
    if (
      state.selectedTaskGroupId
      && !payload.groups.some((group) => group.task_group_id === state.selectedTaskGroupId)
    ) {
      state.selectedTaskGroupId = null;
      state.selectedGroupDetail = null;
    }
  }

  function applyDetailSnapshot(payload: { task_group_id: string; group: TaskGroup }) {
    state.groupDetails[payload.task_group_id] = payload.group;
    if (payload.task_group_id === state.selectedTaskGroupId) {
      state.selectedGroupDetail = payload.group;
    }
  }

  function appendTaskLog(entry: TaskLogEntry) {
    state.taskLogs.push(entry);
    if (state.taskLogs.length > MAX_TASK_LOG_ENTRIES) {
      state.taskLogs.splice(0, state.taskLogs.length - MAX_TASK_LOG_ENTRIES);
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
      state.groupDetails[handle.task_group_id] = await api.getTaskGroupDetail(handle.task_group_id);
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
