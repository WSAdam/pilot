// Loads a corpus of public sites in pilot the way an agent would (goto, the
// AI snapshot behind MCP's browser_snapshot, then click a link) and records
// what works, for compat/open-web.json. A measurement, not a test: the web
// changes, and failures are leads to investigate.
//
// A site that wedges the browser (Build 11 lost it on IMDb, whose context
// close never returned, and in a later run waited 36 minutes on its
// page.title) is recorded as such once it passes SITE_LIMIT_MS or its
// context close passes CLOSE_TIMEOUT_MS, and pilot is relaunched for the
// next site, so one site can't end the run.
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
const CLOSE_TIMEOUT_MS = 15_000;
const SITE_LIMIT_MS = 120_000;

/** Resolves to `promise`'s value, or to `fallback` once `ms` pass. */
function within(promise, ms, fallback) {
  let timer;
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise((done) => { timer = setTimeout(() => done(fallback), ms); }),
  ]);
}

/** pilot behind a launch server, so a wedged browser can be killed. */
async function launch() {
  const server = await chromium.launchServer({ executablePath: pilot, headless: true, env: noWindowEnv });
  const browser = await chromium.connect(server.wsEndpoint());
  return { server, browser };
}

async function visit(browser, url) {
  const started = performance.now();
  const result = { url, loaded: false, snapshot: 0, links: 0, clicked: false };
  let context;
  let page;
  try {
    context = await browser.newContext();
    page = await context.newPage();
  } catch (error) {
    result.error = error.message.split('\n')[0].slice(0, 200);
    result.browserLost = true;
    result.ms = Math.round(performance.now() - started);
    return result;
  }
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
    const closed = await within(context.close().then(() => 'closed', () => 'browser lost'), CLOSE_TIMEOUT_MS, 'wedged');
    if (closed === 'wedged') {
      result.wedged = true;
      result.error = `${result.error ? `${result.error}; ` : ''}context.close did not return in ${CLOSE_TIMEOUT_MS / 1000} s`;
    } else if (closed === 'browser lost') {
      result.browserLost = true;
    }
    result.ms = Math.round(performance.now() - started);
  }
  return result;
}

let pilotProcess = await launch();
const results = [];
for (const url of SITES) {
  const started = performance.now();
  const timedOut = () => ({
    url, loaded: false, snapshot: 0, links: 0, clicked: false, wedged: true,
    error: `the site did not finish in ${SITE_LIMIT_MS / 1000} s`, ms: Math.round(performance.now() - started),
  });
  let result = await within(visit(pilotProcess.browser, url), SITE_LIMIT_MS, null) ?? timedOut();
  if (result.browserLost && !result.loaded) {
    // The browser was already gone before this site loaded: relaunch and
    // give the site its own try.
    await pilotProcess.server.kill();
    pilotProcess = await launch();
    result = { ...(await within(visit(pilotProcess.browser, url), SITE_LIMIT_MS, null) ?? timedOut()), relaunchedBefore: true };
  }
  results.push(result);
  const mark = result.wedged ? '❌' : result.loaded && result.snapshot > 200 && result.clicked ? '✅' : result.loaded ? '⚠️' : '❌';
  console.log(`${mark} ${url} — ${result.status ?? '-'} snapshot ${result.snapshot} links ${result.links} clicked ${result.clicked} ${result.ms}ms${result.error ? ` — ${result.error}` : ''}`);
  if (result.wedged || result.browserLost) {
    console.log(`⚠️ pilot ${result.wedged ? 'wedged' : 'was lost'} on ${url}; relaunching`);
    await pilotProcess.server.kill();
    pilotProcess = await launch();
  }
}
await within(pilotProcess.browser.close(), CLOSE_TIMEOUT_MS).catch(() => {});
await pilotProcess.server.kill();

const usable = results.filter((result) => !result.wedged && result.loaded && result.snapshot > 200 && result.clicked).length;
const loaded = results.filter((result) => result.loaded).length;
const wedged = results.filter((result) => result.wedged || result.browserLost).length;
const report = { date: new Date().toISOString().slice(0, 10), pilot, sites: results.length, loaded, usable, wedged, results };
const compat = resolve(here, '../../../compat');
await mkdir(compat, { recursive: true });
await writeFile(resolve(compat, 'open-web.json'), `${JSON.stringify(report, null, 2)}\n`);
console.log(`🔍 ${usable}/${results.length} sites loaded, snapshotted and clicked through (${loaded} loaded, ${wedged} wedged or lost the browser); wrote compat/open-web.json`);
