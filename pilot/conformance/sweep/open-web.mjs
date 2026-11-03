// Loads a corpus of public sites in pilot the way an agent would (goto, the
// AI snapshot behind MCP's browser_snapshot, then click a link) and records
// what works, for compat/open-web.json. A measurement, not a test: the web
// changes, and failures are leads to investigate.
//
// Usage: npm run open-web  (PILOT_BIN selects the binary; default release build)
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { noWindowEnv } from '../support.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const pilot = process.env.PILOT_BIN ?? resolve(here, '../../../target/release/pilot');
const SITES = [
  'https://en.wikipedia.org/wiki/Web_browser',
  'https://news.ycombinator.com/',
  'https://github.com/microsoft/playwright',
  'https://developer.mozilla.org/en-US/docs/Web/HTML',
  'https://docs.python.org/3/',
  'https://www.rust-lang.org/',
  'https://deno.com/',
  'https://nodejs.org/en',
  'https://www.npmjs.com/package/playwright',
  'https://crates.io/',
  'https://jsr.io/',
  'https://stackoverflow.com/questions',
  'https://www.bbc.com/news',
  'https://www.theguardian.com/international',
  'https://www.reuters.com/',
  'https://www.amazon.com/',
  'https://www.ebay.com/',
  'https://www.etsy.com/',
  'https://www.youtube.com/',
  'https://www.google.com/',
  'https://duckduckgo.com/',
  'https://www.bing.com/',
  'https://www.apple.com/',
  'https://www.microsoft.com/en-us/',
  'https://www.cloudflare.com/',
  'https://archive.org/',
  'https://www.imdb.com/',
  'https://www.weather.gov/',
  'https://www.craigslist.org/',
  'https://www.reddit.com/',
];
const NAVIGATION_TIMEOUT_MS = 30_000;

async function visit(browser, url) {
  const started = performance.now();
  const result = { url, loaded: false, snapshot: 0, links: 0, clicked: false };
  const context = await browser.newContext();
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(error.message.slice(0, 120)));
  try {
    const response = await page.goto(url, { timeout: NAVIGATION_TIMEOUT_MS, waitUntil: 'load' });
    result.status = response?.status();
    result.loaded = true;
    // An agent snapshots once the page settles; single-page apps render
    // after load.
    await page.waitForLoadState('networkidle', { timeout: 5_000 }).catch(() => {});
    result.title = (await page.title()).slice(0, 80);
    const snapshot = await page.ariaSnapshot({ mode: 'ai', timeout: 15_000 });
    result.snapshot = snapshot.length;
    result.links = (snapshot.match(/- link /g) ?? []).length;
    const link = page.locator('a[href^="http"], a[href^="/"]').filter({ visible: true }).first();
    const before = page.url();
    await link.click({ timeout: 10_000 });
    await page.waitForLoadState('load', { timeout: NAVIGATION_TIMEOUT_MS }).catch(() => {});
    result.clicked = true;
    result.navigatedTo = page.url() === before ? '(same page)' : page.url().slice(0, 100);
  } catch (error) {
    result.error = error.message.split('\n')[0].slice(0, 200);
  } finally {
    result.pageErrors = pageErrors.slice(0, 3);
    result.ms = Math.round(performance.now() - started);
    await context.close().catch(() => {});
  }
  return result;
}

const browser = await chromium.launch({ executablePath: pilot, headless: true, env: noWindowEnv });
const results = [];
for (const url of SITES) {
  const result = await visit(browser, url);
  results.push(result);
  const mark = result.loaded && result.snapshot > 200 && result.clicked ? '✅' : result.loaded ? '⚠️' : '❌';
  console.log(`${mark} ${url} — ${result.status ?? '-'} snapshot ${result.snapshot} links ${result.links} clicked ${result.clicked} ${result.ms}ms${result.error ? ` — ${result.error}` : ''}`);
}
await browser.close();

const usable = results.filter((result) => result.loaded && result.snapshot > 200 && result.clicked).length;
const loaded = results.filter((result) => result.loaded).length;
const report = { date: new Date().toISOString().slice(0, 10), pilot, sites: results.length, loaded, usable, results };
const compat = resolve(here, '../../../compat');
await mkdir(compat, { recursive: true });
await writeFile(resolve(compat, 'open-web.json'), `${JSON.stringify(report, null, 2)}\n`);
console.log(`🔍 ${usable}/${results.length} sites loaded, snapshotted and clicked through (${loaded} loaded); wrote compat/open-web.json`);
