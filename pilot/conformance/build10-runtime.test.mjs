// Build 10 (2026-11-18): Runtime domain lines left open after Build 9. Each
// test pins Chrome's behaviour (checked against Chromium headless shell 1223
// by pointing PILOT_BIN at it) and was proven red against the release binary
// of Build 9 before its fix was written.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
let fixture;
let browser;

before(async () => {
  fixture = await startFixtureServer();
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
});
after(async () => {
  await browser?.close();
  await fixture?.close();
});

async function withPage(run, contextOptions = {}) {
  const context = await browser.newContext(contextOptions);
  try {
    return await run(await context.newPage(), context);
  } finally {
    await context.close();
  }
}

/** Every Runtime.executionContextCreated a fresh CDP session sees while `run` runs. */
async function createdContexts(page, context, run) {
  const session = await context.newCDPSession(page);
  const created = [];
  session.on('Runtime.executionContextCreated', ({ context: execution }) => created.push(execution));
  await session.send('Runtime.enable');
  await run();
  await page.waitForTimeout(200);
  await session.detach();
  return created.map(({ name, auxData }) => ({ name, type: auxData?.type, isDefault: auxData?.isDefault, frameId: auxData?.frameId }));
}

// 2026-11-18 (TODO line 188): pilot named every main world after its document
// URL ("about:blank", "http://.../greeting.html"). Chrome's main world has an
// empty name, in the top frame and in child frames alike (Blink passes an
// empty human-readable name for the main world to V8's inspector); only
// isolated worlds are named, Playwright's after __playwright_utility_world__.
test('main world execution contexts have an empty name, as in Chrome', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    const initial = await createdContexts(page, context, async () => {});
    const navigated = await createdContexts(page, context, async () => {
      await page.goto(`${fixture.baseUrl}/greeting.html`);
      await page.evaluate(() => new Promise((loaded) => {
        const frame = document.createElement('iframe');
        frame.src = '/late-button.html';
        frame.onload = loaded;
        document.body.append(frame);
      }));
    });
    const contexts = [...initial, ...navigated];
    const main = contexts.filter((execution) => execution.type === 'default');
    const utility = contexts.filter((execution) => execution.type === 'isolated');
    const frames = new Set(main.map((execution) => execution.frameId));
    assert.ok(main.length >= 3 && frames.size >= 2, `want main worlds of about:blank, greeting.html and its iframe: ${JSON.stringify(contexts)}`);
    assert.deepEqual(main.map((execution) => execution.name), main.map(() => ''), `main worlds ${JSON.stringify(main)}`);
    assert.ok(main.every((execution) => execution.isDefault === true), `main worlds ${JSON.stringify(main)}`);
    assert.ok(utility.length > 0 && utility.every((execution) => execution.name.startsWith('__playwright_utility_world_')), `isolated worlds ${JSON.stringify(utility)}`);
  }));

/** A JavaScript comment the size of Playwright's injected script, full of the quotes, backslashes and newlines JSON escapes. */
const INJECTED_SCRIPT_SIZED_PADDING = '/* "padding" \\ */\n'.repeat(19_000);

// 2026-11-18 guard (TODO line 189, already true on the Build 9 binary): the
// renderer used to parse a Runtime.evaluate into JSON about seven times to read
// its userGesture, its contextId and its method. It now reads them once from
// the command's head; a 346 KB evaluate must still run with its user gesture
// and in the isolated world it names, as in Chrome.
test('a 346 KB Runtime.evaluate keeps its user gesture and its isolated world', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.setContent('<p>large evaluate</p>');
    await page.evaluate(() => { window.mainWorldOnly = 'main'; });
    const session = await context.newCDPSession(page);
    const { frameTree } = await session.send('Page.getFrameTree');
    const { executionContextId } = await session.send('Page.createIsolatedWorld', { frameId: frameTree.frame.id, worldName: 'build10-large-evaluate' });
    const gesture = await session.send('Runtime.evaluate', {
      expression: `${INJECTED_SCRIPT_SIZED_PADDING}navigator.userActivation.isActive`,
      userGesture: true,
      returnByValue: true,
    });
    const isolated = await session.send('Runtime.evaluate', {
      expression: `${INJECTED_SCRIPT_SIZED_PADDING}typeof window.mainWorldOnly + ' ' + document.querySelector('p').textContent`,
      contextId: executionContextId,
      returnByValue: true,
    });
    await session.detach();
    assert.equal(gesture.result.value, true, `userGesture evaluate ${JSON.stringify(gesture)}`);
    assert.equal(isolated.result.value, 'undefined large evaluate', `isolated evaluate ${JSON.stringify(isolated)}`);
  }));
