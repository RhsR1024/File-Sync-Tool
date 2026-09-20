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
assert.equal(Object.keys(store.groupDetails).length, 0);
assert.equal(Object.keys(store.taskLogsByGroup).length, 0);
assert.equal(store.taskLogs.length, 0);

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

// An old query must not put deleted/completed tasks back after a live snapshot.
const oldList = deferred();
let listCalls = 0;
const raceStore = createTaskStateStore({ listTaskGroups: () => { listCalls++; return oldList.promise; } });
const firstHydrate = raceStore.hydrateTaskState();
assert.equal(raceStore.hydrateTaskState(), firstHydrate);
await Promise.resolve();
raceStore.applyGroupsSnapshot({ revision: 10, groups: [{ ...sampleGroup, summary_status: 'completed' }] });
oldList.resolve([sampleGroup]);
await firstHydrate;
assert.equal(listCalls, 1);
assert.equal(raceStore.isHydrated, true);
assert.equal(raceStore.groups[0].summary_status, 'completed');
raceStore.applyGroupsSnapshot({ revision: 9, groups: [] });
assert.equal(raceStore.groups.length, 1);

// Switching selection and receiving detail events cannot be undone by slow IPC.
const firstDetail = deferred();
const secondDetail = deferred();
const detailStore = createTaskStateStore({
  getTaskGroupDetail: id => id === 'group-1' ? firstDetail.promise : secondDetail.promise,
});
const firstSelection = detailStore.selectTaskGroup('group-1');
const secondSelection = detailStore.selectTaskGroup('group-2');
firstDetail.resolve(sampleDetail);
await firstSelection;
assert.equal(detailStore.selectedTaskGroupId, 'group-2');
assert.equal(detailStore.isLoadingDetail, true);
detailStore.applyDetailSnapshot({ task_group_id: 'group-2', group: { ...sampleDetail, task_group_id: 'group-2', summary_status: 'completed' } });
secondDetail.resolve({ ...sampleDetail, task_group_id: 'group-2' });
await secondSelection;
assert.equal(detailStore.selectedGroupDetail.summary_status, 'completed');
assert.equal(detailStore.isLoadingDetail, false);

// Even without list snapshots, long-running task churn has a fixed cache budget.
const churnStore = createTaskStateStore();
for (let i = 0; i < 10_100; i++) {
  const id = `churn-${i}`;
  churnStore.appendTaskLog({ task_group_id: id, run_id: `run-${i}`, level: 'info', message: 'log', timestamp: 'now', server_id: null, server_name: null });
  if (i < 100) churnStore.applyDetailSnapshot({ task_group_id: id, group: { ...sampleDetail, task_group_id: id } });
}
assert.ok(churnStore.taskLogs.length <= 10_000 && churnStore.taskLogs.length >= 9_000);
assert.equal(Object.keys(churnStore.taskLogsByGroup).length, churnStore.taskLogs.length);
assert.equal(Object.values(churnStore.taskLogsByGroup).reduce((sum, logs) => sum + logs.length, 0), churnStore.taskLogs.length);
assert.ok(Object.keys(churnStore.groupDetails).length <= 32);
assert.equal(churnStore.taskLogsByGroup['churn-0'], undefined);
churnStore.applyGroupsSnapshot({ groups: [] });
assert.equal(Object.keys(churnStore.taskLogsByGroup).length, 0);
assert.equal(Object.keys(churnStore.groupDetails).length, 0);

// Mutating bounded arrays still invalidates Vue consumers of shallow store state.
const { computed } = await import('vue');
const logCount = computed(() => churnStore.taskLogs.length);
const groupLogCount = computed(() => churnStore.taskLogsByGroup['same-group']?.length ?? 0);
assert.equal(logCount.value, 0);
assert.equal(groupLogCount.value, 0);
for (let i = 0; i < 2_100; i++) {
  churnStore.appendTaskLog({ task_group_id: 'same-group', run_id: 'same-run', level: 'info', message: `${i}`, timestamp: 'now', server_id: null, server_name: null });
}
assert.equal(logCount.value, 2_100);
assert.ok(groupLogCount.value <= 2_000 && groupLogCount.value >= 1_800);
assert.equal(churnStore.taskLogsByGroup['same-group'].at(-1).message, '2099');

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
