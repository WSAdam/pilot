// pilot: run with `node --test scripts/precheck-source-boundary.test.mjs`
// from the repository root.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { boundaryCountChanges } from './precheck-source-boundary.mjs';

// 2026-11-19: Build 10's first gate run (44380599045) failed
// script_and_module_execution_use_the_shared_entry_points because the page
// error unit test in util.rs ran its script with `.run(&scope)` directly.
// 6db71df64 is main before Build 10; 9a95b78b1 is the head that run tested.
test('a renderer file gaining a direct script run is reported, as Build 10 run 1 failed', () => {
  const { changes } = boundaryCountChanges('6db71df64', '9a95b78b1');
  assert.deepEqual(changes, [{ inventory: 'scriptRuns', file: 'moli-renderer-v8/src/util.rs', from: 0, to: 1 }]);
});

// 50656e200 moved that test onto script_execution::execute_compiled_script,
// and the Linux job passed the boundary test from then on.
test('the fixed Build 10 head moves no count', () => {
  assert.deepEqual(boundaryCountChanges('6db71df64', '50656e200').changes, []);
});
