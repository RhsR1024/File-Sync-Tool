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
assert.equal(store.groupDetails['group-1'].task_group_id, 'group-1');

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

// ── Manual task action tests ─────────────────────────────────────────────────
const actionCalls = [];
const actionStore = createTaskStateStore({
  listTaskGroups: async () => [],
  getTaskGroupDetail: async () => sampleDetail,
  startManualCopyTask: async (request) => {
    actionCalls.push(['copy', request]);
    return { task_group_id: 'group-copy', run_id: 'run-copy' };
  },
  startManualDeployTask: async (request) => {
    actionCalls.push(['deploy', request]);
    return { task_group_id: 'group-deploy', run_id: 'run-deploy' };
  },
});

const copyHandle = await actionStore.startManualCopy({
  source_path: 'C:\\src\\pkg',
  target_root_path: 'D:\\dst',
  overwrite_existing: false,
  file_extensions: ['.zip'],
  filename_includes: ['pkg'],
});

assert.equal(copyHandle.task_group_id, 'group-copy');
assert.equal(copyHandle.run_id, 'run-copy');

const deployHandle = await actionStore.startManualDeploy({
  task_group_id: null,
  display_name: 'pkg',
  local_path: 'D:\\dst\\pkg',
  remote_path: '/srv/pkg',
  transfer_policy: 'smart',
  extract_policy: 'auto',
  extract_dir: '${remote_target}/${filename}',
  bindings: [{
    server_id: 'server-a',
    command_group_ids: ['extract', 'install'],
    extract_command_group_id: 'extract',
  }],
});

assert.equal(deployHandle.task_group_id, 'group-deploy');
assert.equal(deployHandle.run_id, 'run-deploy');
assert.equal(actionStore.latestManualDeploy.task_group_id, 'group-deploy');
assert.equal(actionStore.latestManualDeploy.run_id, 'run-deploy');
assert.deepEqual(actionStore.latestManualDeploy.server_ids, ['server-a']);
assert.equal(actionCalls.length, 2);
assert.equal(actionCalls[0][0], 'copy');
assert.equal(actionCalls[1][0], 'deploy');
assert.deepEqual(actionCalls[1][1], {
  task_group_id: null,
  display_name: 'pkg',
  local_path: 'D:\\dst\\pkg',
  remote_path: '/srv/pkg',
  transfer_policy: 'smart',
  extract_policy: 'auto',
  extract_dir: '${remote_target}/${filename}',
  bindings: [{
    server_id: 'server-a',
    command_group_ids: ['extract', 'install'],
    extract_command_group_id: 'extract',
  }],
});
console.log('taskStateStore manual action tests PASSED');
