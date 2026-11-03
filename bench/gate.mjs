// Fails a bench run whose medians exceed bench/budgets.json, so startup,
// memory and snapshot latency cannot regress unnoticed.
//
// Usage: node gate.mjs [results/<file>.json]  (default: the newest result)
import { readdir, readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

/** Each budgeted metric with its measured median and whether it fits. */
export function checkBudgets(result, budgets) {
  return Object.entries(budgets).map(([metric, budget]) => {
    const measured = metric.split('.').reduce((node, key) => node?.[key], result);
    const value = typeof measured === 'object' && measured !== null ? measured.median : measured;
    return { metric, value, budget, ok: typeof value === 'number' && value <= budget };
  });
}

async function newestResult() {
  const files = (await readdir(new URL('./results/', import.meta.url))).filter((name) => name.endsWith('.json')).sort();
  if (!files.length) throw new Error('no bench results; run node bench.mjs first');
  return new URL(`./results/${files.at(-1)}`, import.meta.url);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const file = process.argv[2] ? pathToFileURL(process.argv[2]) : await newestResult();
  const result = JSON.parse(await readFile(file, 'utf8'));
  const { budgets } = JSON.parse(await readFile(new URL('./budgets.json', import.meta.url), 'utf8'));
  const rows = checkBudgets(result, budgets);
  console.log(`🔍 ${file.pathname.split('/').at(-1)} (${result.label}, ${result.commit})`);
  for (const row of rows) console.log(`${row.ok ? '✅' : '❌'} ${row.metric}: ${row.value ?? 'missing'} (budget ${row.budget})`);
  const failed = rows.filter((row) => !row.ok).length;
  console.log(failed ? `❌ ${failed} metric(s) over budget` : '✅ every metric within budget');
  process.exitCode = failed ? 1 : 0;
}
