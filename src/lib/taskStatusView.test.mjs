import assert from 'node:assert/strict';
import { buildTaskRows, buildTaskDetailSections } from './taskStatusView.ts';

const rows = buildTaskRows([
  {
    task_group_id: 'group-2',
    display_name: 'newer',
    folder_name: 'newer',
    source_path: 'C:\\src\\newer',
    local_target_path: 'D:\\dst\\newer',
    summary_status: 'deploying',
    copy_status: 'completed',
    deploy_status: 'running',
    started_at: '2026-04-02T12:10:00+08:00',
    finished_at: null,
    elapsed_seconds: 40,
    latest_run_id: 'run-2',
    had_failures: false,
    merge_key: 'scheduled||d:\\dst\\newer||newer',
    task_config_id: 'task-a',
    server_rollups: [],
  },
  {
    task_group_id: 'group-1',
    display_name: 'older',
    folder_name: 'older',
    source_path: 'C:\\src\\older',
    local_target_path: 'D:\\dst\\older',
    summary_status: 'completed',
    copy_status: 'completed',
    deploy_status: 'completed',
    started_at: '2026-04-02T11:10:00+08:00',
    finished_at: '2026-04-02T11:20:00+08:00',
    elapsed_seconds: 600,
    latest_run_id: 'run-1',
    had_failures: false,
    merge_key: 'scheduled||d:\\dst\\older||older',
    task_config_id: 'task-a',
    server_rollups: [],
  },
]);

assert.equal(rows[0].task_group_id, 'group-2', 'newer group should be first');

const detail = buildTaskDetailSections({
  task_group_id: 'group-2',
  merge_key: 'scheduled||d:\\dst\\newer||newer',
  task_config_id: 'task-a',
  source_type: 'scheduled',
  display_name: 'newer',
  folder_name: 'newer',
  source_path: 'C:\\src\\newer',
  local_target_path: 'D:\\dst\\newer',
  copy_status: 'completed',
  deploy_status: 'partial_failed',
  summary_status: 'partial_failed',
  started_at: '2026-04-02T12:10:00+08:00',
  finished_at: null,
  elapsed_seconds: 40,
  latest_run_id: 'run-2',
  had_failures: true,
  server_rollups: [
    {
      server_id: 'server-a',
      server_name: 'Server A',
      latest_status: 'failed',
      latest_attempt_id: 'attempt-1',
      success_count: 0,
      failure_count: 1,
      last_error_message: 'ssh timeout',
      attempt_ids: ['attempt-1'],
    },
  ],
  runs: [],
});

assert.equal(detail.serverFailures[0].message, 'ssh timeout');
console.log('taskStatusView tests PASSED');
