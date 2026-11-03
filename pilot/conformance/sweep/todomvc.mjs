// Runs the TodoMVC flows in every framework's hosted example on pilot, for
// compat/todomvc.json (the v1 suite's SPA check). Role-based locators, as an
// agent's MCP refs would be, so one flow covers every implementation.
//
// Usage: npm run todomvc  (PILOT_BIN selects the binary; default release build)
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { noWindowEnv } from '../support.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const pilot = process.env.PILOT_BIN ?? resolve(here, '../../../target/release/pilot');
const BASE = 'https://todomvc.com/examples';
const APPS = {
  react: `${BASE}/react/dist/`,
  vue: `${BASE}/vue/dist/`,
  angular: `${BASE}/angular/dist/browser/`,
  svelte: `${BASE}/svelte/dist/`,
  preact: `${BASE}/preact/dist/`,
  lit: `${BASE}/lit/dist/`,
  ember: `${BASE}/emberjs/todomvc/dist/`,
  backbone: `${BASE}/backbone/dist/`,
  jquery: `${BASE}/jquery/dist/`,
  'vanilla es6': `${BASE}/javascript-es6/dist/`,
  knockout: `${BASE}/knockoutjs/`,
};
const STEP_TIMEOUT_MS = 10_000;

async function expectCount(locator, expected, what) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  let count;
  while (Date.now() < deadline) {
    count = await locator.count();
    if (count === expected) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error(`${what}: ${count} items, expected ${expected}`);
}

async function runFlow(page, url) {
  const steps = [];
  const step = async (name, run) => {
    try {
      await run();
      steps.push({ step: name, ok: true });
    } catch (error) {
      steps.push({ step: name, ok: false, error: error.message.split('\n')[0].slice(0, 160) });
      throw error;
    }
  };
  const input = page.getByPlaceholder('What needs to be done?');
  const items = page.getByRole('listitem').filter({ has: page.getByRole('checkbox') });
  try {
    await step('load', async () => {
      await page.goto(url, { timeout: 30_000 });
      await input.waitFor({ timeout: STEP_TIMEOUT_MS });
    });
    await step('add three todos', async () => {
      for (const text of ['Buy milk', 'Walk the dog', 'Write code']) {
        await input.fill(text, { timeout: STEP_TIMEOUT_MS });
        await input.press('Enter');
      }
      await expectCount(items, 3, 'after adding');
    });
    await step('complete one', async () => {
      await items.filter({ hasText: 'Buy milk' }).getByRole('checkbox').check({ timeout: STEP_TIMEOUT_MS });
    });
    await step('filter Active', async () => {
      await page.getByRole('link', { name: 'Active', exact: true }).first().click({ timeout: STEP_TIMEOUT_MS });
      await expectCount(items, 2, 'Active');
    });
    await step('filter Completed', async () => {
      await page.getByRole('link', { name: 'Completed', exact: true }).first().click({ timeout: STEP_TIMEOUT_MS });
      await expectCount(items, 1, 'Completed');
    });
    await step('filter All', async () => {
      await page.getByRole('link', { name: 'All', exact: true }).first().click({ timeout: STEP_TIMEOUT_MS });
      await expectCount(items, 3, 'All');
    });
    await step('clear completed', async () => {
      await page.getByRole('button', { name: /Clear completed/i }).click({ timeout: STEP_TIMEOUT_MS });
      await expectCount(items, 2, 'after clearing');
    });
    await step('edit by double-click', async () => {
      await items.filter({ hasText: 'Walk the dog' }).getByText('Walk the dog').dblclick({ timeout: STEP_TIMEOUT_MS });
      const editor = page.locator('input.edit:visible, input[type="text"]:focus').first();
      await editor.fill('Walk the cat', { timeout: STEP_TIMEOUT_MS });
      await editor.press('Enter');
      await items.filter({ hasText: 'Walk the cat' }).first().waitFor({ timeout: STEP_TIMEOUT_MS });
    });
  } catch {
    // The failing step is recorded; later steps depend on it.
  }
  return steps;
}

const browser = await chromium.launch({ executablePath: pilot, headless: true, env: noWindowEnv });
const results = [];
for (const [app, url] of Object.entries(APPS)) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const steps = await runFlow(page, url);
  await context.close();
  const ok = steps.length === 8 && steps.every((step) => step.ok);
  const failed = steps.find((step) => !step.ok);
  results.push({ app, url, ok, steps });
  console.log(`${ok ? '✅' : '❌'} ${app}${failed ? ` — ${failed.step}: ${failed.error}` : ''}`);
}
await browser.close();

const passed = results.filter((result) => result.ok).length;
const report = { date: new Date().toISOString().slice(0, 10), pilot, passed, total: results.length, results };
const compat = resolve(here, '../../../compat');
await mkdir(compat, { recursive: true });
await writeFile(resolve(compat, 'todomvc.json'), `${JSON.stringify(report, null, 2)}\n`);
console.log(`🔍 ${passed}/${results.length} TodoMVC apps pass every flow; wrote compat/todomvc.json`);
