import assert from 'node:assert/strict';

import { createTaskStateStore } from './taskStateStore.ts';

const sampleGroup = {
  task_group_id: 'group-1',
  display_name: 'pkg',
  folder_name: 'pkg',
  source_path: 'C:\\src\\pkg',
  local_target_path: 'D:\\dst\\pkg',
  summary_status: 'copying',
  copy_status: 'running',
  deploy_status: 'not_started',
  started_at: '2026-04-02T12:00:00+08:00',
  finished_at: null,
  elapsed_seconds: 12,
  latest_run_id: 'run-1',
  had_failures: false,
  merge_key: 'manual||d:\\dst\\pkg||pkg',
  task_config_id: null,
  server_rollups: [],
};

const sampleDetail = {
  task_group_id: 'group-1',
  merge_key: 'manual||d:\\dst\\pkg||pkg',
  task_config_id: null,
  source_type: 'manual',
  display_name: 'pkg',
  folder_name: 'pkg',
  source_path: 'C:\\src\\pkg',
  local_target_path: 'D:\\dst\\pkg',
  copy_status: 'running',
  deploy_status: 'not_started',
  summary_status: 'copying',
  started_at: '2026-04-02T12:00:00+08:00',
  finished_at: null,
  elapsed_seconds: 12,
  latest_run_id: 'run-1',
  had_failures: false,
  server_rollups: [],
  runs: [],
};

const api = {
  listTaskGroups: async () => [sampleGroup],
  getTaskGroupDetail: async () => sampleDetail,
};

const store = createTaskStateStore(api);
await store.hydrateTaskState();
assert.equal(store.groups[0].task_group_id, 'group-1');

await store.selectTaskGroup('group-1');
assert.equal(store.selectedTaskGroupId, 'group-1');
assert.equal(store.selectedGroupDetail.task_group_id, 'group-1');

store.applyDetailSnapshot({
  task_group_id: 'group-1',
  group: {
    ...sampleDetail,
    summary_status: 'completed',
  },
});
assert.equal(store.selectedGroupDetail.summary_status, 'completed');

store.appendTaskLog({
  task_group_id: 'group-1',
  run_id: 'run-1',
  server_id: null,
  server_name: null,
  level: 'info',
  message: 'copy started',
  timestamp: '2026-04-02T12:00:01+08:00',
});
assert.equal(store.taskLogs.length, 1);

store.applyGroupsSnapshot({ groups: [] });
assert.equal(store.groups.length, 0);
assert.equal(store.selectedTaskGroupId, null);
assert.equal(store.selectedGroupDetail, null);
