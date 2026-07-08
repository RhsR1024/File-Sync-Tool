import assert from 'node:assert/strict';

import { getApplianceSshEnableState, isValidSshPort } from './applianceSshPresentation.ts';

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

assert.equal(isValidSshPort(23333), true, '23333 is a valid SSH port');
assert.equal(isValidSshPort(1), true, '1 is a valid SSH port');
assert.equal(isValidSshPort(65535), true, '65535 is a valid SSH port');
assert.equal(isValidSshPort(0), false, '0 is not a valid SSH port');
assert.equal(isValidSshPort(70000), false, '70000 is out of range');
assert.equal(isValidSshPort(1.5), false, 'non-integer is invalid');
assert.equal(isValidSshPort(Number.NaN), false, 'NaN is invalid');
assert.equal(isValidSshPort('23333'), false, 'string is invalid');

console.log('applianceSshPresentation tests PASSED');
