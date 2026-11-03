// The bench gate fails a run when a measured median exceeds its budget.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { checkBudgets } from './gate.mjs';

const budgets = { 'startup.readyMs': 60, 'memory.perExtraContextMiB': 20, 'geometry.lateClickMs': 120 };
const result = (readyMs, perExtraContextMiB, lateClickMs) => ({
  startup: { readyMs: { median: readyMs } },
  memory: { perExtraContextMiB },
  geometry: { lateClickMs: { median: lateClickMs } },
});

test('a run within every budget passes', () => {
  assert.deepEqual(checkBudgets(result(27, 11.3, 59), budgets).filter((row) => !row.ok), []);
});

test('each metric over its budget is reported', () => {
  const over = checkBudgets(result(75, 11.3, 130), budgets).filter((row) => !row.ok);
  assert.deepEqual(over.map((row) => [row.metric, row.value, row.budget]), [
    ['startup.readyMs', 75, 60],
    ['geometry.lateClickMs', 130, 120],
  ]);
});

test('a metric missing from the run fails rather than passing silently', () => {
  const rows = checkBudgets({ startup: {} }, { 'startup.readyMs': 60 });
  assert.deepEqual(rows.map((row) => [row.ok, row.value]), [[false, undefined]]);
});
