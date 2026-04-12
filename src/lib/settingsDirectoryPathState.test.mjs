import assert from 'node:assert/strict';

import {
  getDirectoryInputValue,
  getTaskLocalPathHint,
  getTaskLocalPathPlaceholder,
  toOptionalDirectoryValue,
} from './settingsDirectoryPathState.ts';

assert.equal(getDirectoryInputValue(null), '');
assert.equal(getDirectoryInputValue('D:\\Builds'), 'D:\\Builds');

assert.equal(toOptionalDirectoryValue(''), null);
assert.equal(toOptionalDirectoryValue('   '), null);
assert.equal(toOptionalDirectoryValue(' D:\\Builds '), 'D:\\Builds');

assert.equal(getTaskLocalPathPlaceholder('D:\\GlobalTarget'), 'D:\\GlobalTarget');
assert.equal(getTaskLocalPathPlaceholder(''), '');

assert.equal(
  getTaskLocalPathHint('Use Local Storage target directory'),
  'Use Local Storage target directory',
);

console.log('settingsDirectoryPathState tests PASSED');
