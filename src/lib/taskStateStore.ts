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

export function createTaskStateStore(apiOverrides: Partial<TaskStateStoreApi> = {}) {
  const api = { ...defaultApi, ...apiOverrides };
  const state = reactive({
    groups: [] as TaskGroupListItem[],
    selectedTaskGroupId: null as string | null,
    selectedGroupDetail: null as TaskGroup | null,
    isHydrated: false,
    isLoadingDetail: false,
    taskLogs: [] as TaskLogEntry[],
  });

  async function hydrateTaskState() {
    state.groups = await api.listTaskGroups();
    state.isHydrated = true;
  }

  async function selectTaskGroup(taskGroupId: string) {
    state.selectedTaskGroupId = taskGroupId;
    state.isLoadingDetail = true;
    try {
      state.selectedGroupDetail = await api.getTaskGroupDetail(taskGroupId);
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
    if (payload.task_group_id === state.selectedTaskGroupId) {
      state.selectedGroupDetail = payload.group;
    }
  }

  function appendTaskLog(entry: TaskLogEntry) {
    state.taskLogs.push(entry);
  }

  async function startManualCopy(request: StartManualCopyTaskRequest) {
    const handle = await api.startManualCopyTask(request);
    await hydrateTaskState();
    return handle;
  }

  async function startManualDeploy(request: StartManualDeployTaskRequest) {
    const handle = await api.startManualDeployTask(request);
    await hydrateTaskState();
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
