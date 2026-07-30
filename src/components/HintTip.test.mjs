import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const component = readFileSync(new URL('./HintTip.vue', import.meta.url), 'utf8');

assert.match(component, /<Teleport to="body">/, 'tooltips must escape clipping containers');
assert.match(component, /pointer-events-none fixed z-\[9999\]/, 'tooltips must use a viewport-level layer');
assert.match(component, /window\.innerWidth/, 'horizontal placement must use the viewport bounds');
assert.match(component, /window\.innerHeight/, 'vertical placement must use the viewport bounds');
assert.match(component, /const placeAbove =/, 'tooltips must flip above when there is insufficient room below');
assert.match(component, /window\.addEventListener\('scroll', updateTooltipPosition, true\)/, 'tooltips must follow nested scrolling');
assert.match(component, /cursor-pointer/, 'the hint control must use a standard interactive cursor');
assert.doesNotMatch(component, /cursor-help/, 'the cursor must not add a second question mark');
assert.doesNotMatch(component, /:title="text"/, 'the native browser tooltip must not duplicate the app tooltip');

console.log('HintTip contract tests PASSED');
