import assert from 'node:assert/strict';
import { test } from 'node:test';

import { shouldReleaseActiveAlarmJob } from './deviceSimulator.ts';

function status(state, activeAlarmJobs) {
  return {
    state,
    session_id: 'session-1',
    started_at: null,
    phase_progress: null,
    metrics: {
      total_devices: 1,
      online_devices: 1,
      total_channels: 1,
      active_rtsp_clients: 0,
      outbound_bitrate_kbps: 0,
      active_alarm_jobs: activeAlarmJobs,
    },
    cleanup_stage: null,
    recovery_session_id: null,
    last_error: null,
  };
}

test('releases a stale frontend alarm id when the running Worker has no alarm job', () => {
  assert.equal(shouldReleaseActiveAlarmJob(status('running', 0), false), true);
});

test('does not race an alarm start request that has not returned its job id yet', () => {
  assert.equal(shouldReleaseActiveAlarmJob(status('running', 0), true), false);
});

test('keeps an active job and releases alarm state when the runtime is unavailable', () => {
  assert.equal(shouldReleaseActiveAlarmJob(status('running', 1), false), false);
  assert.equal(shouldReleaseActiveAlarmJob(status('stopped', 1), true), true);
});
