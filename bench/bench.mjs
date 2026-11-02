// Benchmarks a moli-compatible browser the way Playwright MCP drives it, over CDP only.
// It never launches Chromium: the browser under test is started as `<bin> serve --layout --port <port>`.
//
// Usage: node bench.mjs --bin <path-to-moli-or-pilot> [--runs 10] [--contexts 25] [--label baseline]
// Writes results/<date>-<label>.json and prints a summary.
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { createServer as createNetServer } from 'node:net';
import { cpus, arch, platform, release, totalmem } from 'node:os';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright-core';

const { values: options } = parseArgs({
  options: {
    bin: { type: 'string' },
    runs: { type: 'string', default: '10' },
    contexts: { type: 'string', default: '25' },
    label: { type: 'string', default: 'run' },
  },
});
if (!options.bin) {
  console.error('❌ --bin <path> is required');
  process.exit(2);
}
const runs = Number(options.runs);
const contextCount = Number(options.contexts);
const fixturesDir = new URL('./fixtures/', import.meta.url);

const median = (values) => {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const round = (value) => Math.round(value * 10) / 10;
const summarize = (values) => ({ median: round(median(values)), min: round(Math.min(...values)), max: round(Math.max(...values)), samples: values.map(round) });
const rssMiB = (pid) => Number(execFileSync('ps', ['-o', 'rss=', '-p', String(pid)]).toString().trim()) / 1024;

async function freePort() {
  const server = createNetServer();
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function startFixtureServer() {
  const server = createServer(async (request, response) => {
    try {
      const body = await readFile(new URL(`.${new URL(request.url, 'http://x').pathname}`, fixturesDir));
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { server, baseUrl: `http://127.0.0.1:${server.address().port}` };
}

async function startBrowser() {
  const port = await freePort();
  const started = performance.now();
  const child = spawn(options.bin, ['serve', '--layout', '--port', String(port)], { stdio: 'ignore' });
  const endpoint = `http://127.0.0.1:${port}`;
  const deadline = started + 15_000;
  while (performance.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`browser exited early with code ${child.exitCode}`);
    try {
      const response = await fetch(`${endpoint}/json/version/`);
      if (response.ok) return { child, endpoint, readyMs: performance.now() - started, version: await response.json() };
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  child.kill('SIGKILL');
  throw new Error('browser did not become ready within 15 s');
}

// Over connectOverCDP, browser.close() only disconnects, so send Browser.close
// explicitly and time until the process exits (as Playwright's launch path does).
async function stopBrowser(browser, child) {
  const started = performance.now();
  const exited = new Promise((resolve) => child.once('exit', resolve));
  let killedAfterTimeout = false;
  const timer = setTimeout(() => {
    killedAfterTimeout = true;
    child.kill('SIGKILL');
  }, 5_000);
  try {
    const session = await browser.newBrowserCDPSession();
    await session.send('Browser.close').catch(() => {});
  } catch (error) {
    console.warn('⚠️ Browser.close could not be sent:', error.message);
  }
  await exited;
  clearTimeout(timer);
  if (killedAfterTimeout) console.warn('⚠️ browser did not exit within 5 s of Browser.close; killed');
  return performance.now() - started;
}

async function measureStartup(pageUrl) {
  const samples = { readyMs: [], connectMs: [], firstSnapshotFromSpawnMs: [], closeMs: [] };
  let version;
  for (let run = 0; run < runs; run++) {
    const spawnedAt = performance.now();
    const { child, endpoint, readyMs, version: reported } = await startBrowser();
    version = reported;
    const connectStarted = performance.now();
    const browser = await chromium.connectOverCDP(endpoint);
    samples.connectMs.push(performance.now() - connectStarted);
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(pageUrl);
    await page.ariaSnapshot({ mode: 'ai' });
    samples.firstSnapshotFromSpawnMs.push(performance.now() - spawnedAt);
    samples.readyMs.push(readyMs);
    samples.closeMs.push(await stopBrowser(browser, child));
    console.log(`🔍 startup run ${run + 1}/${runs}: ready ${round(readyMs)} ms`);
  }
  return { version, ...Object.fromEntries(Object.entries(samples).map(([key, values]) => [key, summarize(values)])) };
}

async function measureWarmPage(pageUrl) {
  const { child, endpoint } = await startBrowser();
  const browser = await chromium.connectOverCDP(endpoint);
  const page = await (await browser.newContext()).newPage();
  const samples = { gotoMs: [], snapshotMs: [], clickMs: [], fillMs: [] };
  try {
    for (let run = 0; run < runs; run++) {
      let started = performance.now();
      await page.goto(pageUrl);
      samples.gotoMs.push(performance.now() - started);
      started = performance.now();
      await page.ariaSnapshot({ mode: 'ai' });
      samples.snapshotMs.push(performance.now() - started);
      started = performance.now();
      await page.getByRole('textbox', { name: 'Email' }).fill('agent@example.com');
      samples.fillMs.push(performance.now() - started);
      started = performance.now();
      await page.getByRole('button', { name: 'Submit' }).click();
      samples.clickMs.push(performance.now() - started);
    }
  } finally {
    await stopBrowser(browser, child);
  }
  return Object.fromEntries(Object.entries(samples).map(([key, values]) => [key, summarize(values)]));
}

async function measureMemory(pageUrl) {
  const { child, endpoint } = await startBrowser();
  const browser = await chromium.connectOverCDP(endpoint);
  try {
    const idleMiB = rssMiB(child.pid);
    const pages = [];
    for (let index = 0; index < contextCount; index++) {
      const page = await (await browser.newContext()).newPage();
      await page.goto(pageUrl);
      await page.ariaSnapshot({ mode: 'ai' });
      pages.push(page);
      if (index === 0) var oneContextMiB = rssMiB(child.pid);
    }
    const allContextsMiB = rssMiB(child.pid);
    return {
      idleRssMiB: round(idleMiB),
      oneContextRssMiB: round(oneContextMiB),
      contexts: contextCount,
      allContextsRssMiB: round(allContextsMiB),
      perExtraContextMiB: round((allContextsMiB - oneContextMiB) / Math.max(1, contextCount - 1)),
    };
  } finally {
    await stopBrowser(browser, child);
  }
}

console.log(`🚀 benchmarking ${options.bin} (${runs} runs, ${contextCount} contexts)`);
const { server, baseUrl } = await startFixtureServer();
const pageUrl = `${baseUrl}/agent-page.html`;
try {
  const startup = await measureStartup(pageUrl);
  console.log('✅ startup measured');
  const warm = await measureWarmPage(pageUrl);
  console.log('✅ warm page actions measured');
  const memory = await measureMemory(pageUrl);
  console.log('✅ memory measured');
  const commit = execFileSync('git', ['rev-parse', '--short', 'HEAD']).toString().trim();
  const result = {
    label: options.label,
    date: new Date().toISOString(),
    commit,
    binary: options.bin,
    browserVersion: startup.version,
    machine: { platform: platform(), release: release(), arch: arch(), cpus: cpus().length, cpuModel: cpus()[0]?.model, totalMemGiB: round(totalmem() / 2 ** 30) },
    node: process.version,
    playwrightCore: (await import('playwright-core/package.json', { with: { type: 'json' } })).default.version,
    startup: { ...startup, version: undefined },
    warm,
    memory,
  };
  await mkdir(new URL('./results/', import.meta.url), { recursive: true });
  const outFile = new URL(`./results/${result.date.slice(0, 10)}-${options.label}.json`, import.meta.url);
  await writeFile(outFile, `${JSON.stringify(result, null, 2)}\n`);
  console.log(`✅ wrote ${outFile.pathname}`);
  console.log(JSON.stringify({ startup: Object.fromEntries(Object.entries(result.startup).filter(([, value]) => value).map(([key, value]) => [key, value.median])), warm: Object.fromEntries(Object.entries(warm).map(([key, value]) => [key, value.median])), memory }, null, 2));
} catch (error) {
  console.error('❌ benchmark failed:', error);
  process.exitCode = 1;
} finally {
  server.close();
}
