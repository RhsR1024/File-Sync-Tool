import assert from 'node:assert/strict';

import { getApplianceSshEnableState } from './applianceSshPresentation.ts';

assert.equal(
  getApplianceSshEnableState(0),
  'disabled',
  'appliance SSH enable=0 should display as disabled, not unknown',
);

assert.equal(
  getApplianceSshEnableState(1),
  'enabled',
  'appliance SSH enable=1 should display as enabled',
);

assert.equal(
  getApplianceSshEnableState(undefined),
  'unknown',
  'missing appliance SSH enable state should display as unknown',
);

console.log('applianceSshPresentation tests PASSED');
