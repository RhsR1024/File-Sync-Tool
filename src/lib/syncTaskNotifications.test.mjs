import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  createSyncTaskNotificationDispatcher,
  createSyncTaskNotificationTracker,
} from './syncTaskNotifications.ts';

function group(overrides = {}) {
  return {
    task_group_id: 'group-1',
    merge_key: 'task-1|D:/sync/build-1|build-1',
    task_config_id: 'task-1',
    display_name: 'build-1',
    folder_name: 'build-1',
    source_path: '//server/share/build-1',
    local_target_path: 'D:/sync/build-1',
    copy_status: 'pending',
    local_exec_status: 'not_started',
    deploy_status: 'not_started',
    summary_status: 'queued',
    started_at: '2026-07-18T10:00:00+08:00',
    finished_at: null,
    elapsed_seconds: 0,
    latest_run_id: 'run-1',
    had_failures: false,
    server_rollups: [],
    ...overrides,
  };
}

test('reports copy, local execution, and deploy milestones once for the same scheduled run', () => {
  const tracker = createSyncTaskNotificationTracker([group()]);

  assert.deepEqual(tracker.collect([group({ copy_status: 'running' })]), [
    { kind: 'copy_started', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({ copy_status: 'completed', deploy_status: 'pending' })]), [
    { kind: 'copy_completed', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'completed',
    local_exec_status: 'running',
    deploy_status: 'pending',
  })]), [
    { kind: 'local_exec_started', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'completed',
    local_exec_status: 'completed',
    deploy_status: 'running',
  })]), [
    { kind: 'local_exec_completed', taskName: 'build-1' },
    { kind: 'deploy_started', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'completed',
    local_exec_status: 'completed',
    deploy_status: 'completed',
  })]), [
    { kind: 'deploy_completed', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'completed',
    local_exec_status: 'completed',
    deploy_status: 'completed',
  })]), []);
});

test('does not notify for hydrated history or manual task groups', () => {
  const completed = group({ copy_status: 'completed', deploy_status: 'completed' });
  const tracker = createSyncTaskNotificationTracker([completed]);

  assert.deepEqual(tracker.collect([completed]), []);
  assert.deepEqual(tracker.collect([group({
    task_group_id: 'manual-group',
    task_config_id: null,
    copy_status: 'running',
  })]), []);
});

test('does not notify when a historical group receives an unqueued scan run', () => {
  const completed = group({ copy_status: 'completed', deploy_status: 'completed' });
  const tracker = createSyncTaskNotificationTracker([completed]);

  assert.deepEqual(tracker.collect([group({
    latest_run_id: 'run-2',
    copy_status: 'pending',
  })]), []);
  assert.deepEqual(tracker.collect([group({
    latest_run_id: 'run-2',
    copy_status: 'running',
  })]), []);
  assert.deepEqual(tracker.collect([group({
    latest_run_id: 'run-2',
    copy_status: 'completed',
    deploy_status: 'completed',
  })]), []);
});

test('allows milestones for a run explicitly announced as queued', () => {
  const completed = group({ copy_status: 'completed', deploy_status: 'completed' });
  const tracker = createSyncTaskNotificationTracker([completed]);

  tracker.markQueued('run-2');
  assert.deepEqual(tracker.collect([group({
    latest_run_id: 'run-2',
    copy_status: 'pending',
  })]), []);
  assert.deepEqual(tracker.collect([group({
    latest_run_id: 'run-2',
    copy_status: 'running',
  })]), [
    { kind: 'copy_started', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    latest_run_id: 'run-2',
    copy_status: 'completed',
    deploy_status: 'completed',
  })]), [
    { kind: 'copy_completed', taskName: 'build-1' },
    { kind: 'deploy_completed', taskName: 'build-1' },
  ]);
});

test('reports terminal failures and falls back to the folder name', () => {
  const tracker = createSyncTaskNotificationTracker([group({ display_name: '' })]);

  assert.deepEqual(tracker.collect([group({ display_name: '', copy_status: 'failed' })]), [
    { kind: 'copy_failed', taskName: 'build-1' },
  ]);

  tracker.remember([group({ copy_status: 'completed', deploy_status: 'running' })]);
  assert.deepEqual(tracker.collect([group({ copy_status: 'completed', deploy_status: 'partial_failed' })]), [
    { kind: 'deploy_failed', taskName: 'build-1' },
  ]);
});

test('reports pause, resume, cancellation, and interruption once', () => {
  const tracker = createSyncTaskNotificationTracker([group({
    copy_status: 'running',
    summary_status: 'copying',
  })]);

  assert.deepEqual(tracker.collect([group({
    copy_status: 'running',
    summary_status: 'paused',
  })]), [
    { kind: 'task_paused', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'running',
    summary_status: 'copying',
  })]), [
    { kind: 'task_resumed', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'cancelled',
    summary_status: 'cancelled',
  })]), [
    { kind: 'task_cancelled', taskName: 'build-1' },
  ]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'cancelled',
    summary_status: 'cancelled',
  })]), []);

  tracker.remember([group({
    copy_status: 'running',
    summary_status: 'copying',
  })]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'interrupted',
    summary_status: 'interrupted',
  })]), [
    { kind: 'task_interrupted', taskName: 'build-1' },
  ]);
});

test('reports local execution failures without mislabelling cancellation as failure', () => {
  const tracker = createSyncTaskNotificationTracker([group({
    copy_status: 'completed',
    local_exec_status: 'running',
    summary_status: 'local_executing',
  })]);

  assert.deepEqual(tracker.collect([group({
    copy_status: 'completed',
    local_exec_status: 'partial_failed',
    summary_status: 'partial_failed',
  })]), [
    { kind: 'local_exec_failed', taskName: 'build-1' },
  ]);

  tracker.remember([group({
    copy_status: 'completed',
    deploy_status: 'running',
    summary_status: 'deploying',
  })]);
  assert.deepEqual(tracker.collect([group({
    copy_status: 'completed',
    deploy_status: 'cancelled',
    summary_status: 'cancelled',
  })]), [
    { kind: 'task_cancelled', taskName: 'build-1' },
  ]);
});

test('delivers native notifications in order without a WebView permission gate', async () => {
  const delivered = [];
  let releaseFirst;
  const firstDelivery = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  const dispatcher = createSyncTaskNotificationDispatcher({
    isEnabled: () => true,
    render: (event) => ({ title: event.kind, body: event.taskName }),
    show: async (title, body) => {
      if (title === 'queued') await firstDelivery;
      delivered.push({ title, body });
    },
    onError: assert.fail,
  });

  const queued = dispatcher.enqueue({ kind: 'queued', taskName: 'build-1' });
  const copying = dispatcher.enqueue({ kind: 'copy_started', taskName: 'build-1' });
  await Promise.resolve();
  assert.deepEqual(delivered, []);
  releaseFirst();
  await Promise.all([queued, copying]);

  assert.deepEqual(delivered, [
    { title: 'queued', body: 'build-1' },
    { title: 'copy_started', body: 'build-1' },
  ]);
});

test('honours the app switch and continues after a native delivery failure', async () => {
  let enabled = false;
  const delivered = [];
  const errors = [];
  const dispatcher = createSyncTaskNotificationDispatcher({
    isEnabled: () => enabled,
    render: (event) => ({ title: event.kind, body: event.taskName }),
    show: async (title) => {
      if (title === 'queued') throw new Error('toast unavailable');
      delivered.push(title);
    },
    onError: (error, event) => errors.push({ error: String(error), kind: event.kind }),
  });

  await dispatcher.enqueue({ kind: 'queued', taskName: 'disabled-run' });
  enabled = true;
  await dispatcher.enqueue({ kind: 'queued', taskName: 'build-1' });
  await dispatcher.enqueue({ kind: 'copy_started', taskName: 'build-1' });

  assert.deepEqual(delivered, ['copy_started']);
  assert.deepEqual(errors, [{ error: 'Error: toast unavailable', kind: 'queued' }]);
});
