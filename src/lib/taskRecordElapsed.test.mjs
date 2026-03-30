import assert from 'node:assert/strict';

import { deriveTaskRecordElapsedSeconds } from './taskRecordElapsed.ts';

const restoredCompletedElapsed = deriveTaskRecordElapsedSeconds({
  phase: 'completed',
  startedAtMs: 1_700_000_000_000,
  updatedAt: 1_700_000_090_000,
  finishedAtMs: 1_700_000_090_000,
  elapsedSeconds: 90,
});

assert.equal(restoredCompletedElapsed, 90);

const remoteStageElapsed = deriveTaskRecordElapsedSeconds(
  {
    phase: 'remote_pushing',
    startedAtMs: 1_700_000_000_000,
    updatedAt: 1_700_000_120_000,
    elapsedSeconds: 120,
  },
  12,
);

assert.equal(remoteStageElapsed, 120);
