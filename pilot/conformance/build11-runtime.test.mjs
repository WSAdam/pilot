// Build 11 (2026-11-18): Runtime domain lines left open after Build 10. Each
// test pins Chrome's behaviour (checked against Chromium headless shell 1223
// by pointing PILOT_BIN at it); a test for a behaviour change was proven red
// against the release binary of Build 9 before its fix was written, and a
// guard passes on both.
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

/** Waits until `find` returns something, polling every 20 ms for up to 5 s. */
async function eventually(find, description) {
  const deadline = Date.now() + 5_000;
  for (;;) {
    const found = find();
    if (found) return found;
    if (Date.now() > deadline) throw new Error(`❌ timed out waiting for ${description}`);
    await new Promise((done) => setTimeout(done, 20));
  }
}

/**
 * The contexts a fresh CDP session sees on Runtime.enable plus one isolated
 * world it creates with grantUniversalAccess in every frame, and the frames.
 */
async function isolatedWorldsOfEveryFrame(page, context, worldName) {
  const session = await context.newCDPSession(page);
  const created = [];
  session.on('Runtime.executionContextCreated', ({ context: execution }) => created.push(execution));
  await session.send('Runtime.enable');
  const { frameTree } = await session.send('Page.getFrameTree');
  const frames = [frameTree.frame, ...(frameTree.childFrames ?? []).map(({ frame }) => frame)];
  for (const frame of frames) {
    await session.send('Page.createIsolatedWorld', { frameId: frame.id, worldName, grantUniversalAccess: true });
  }
  await page.waitForTimeout(200);
  await session.detach();
  return { created, frames };
}

const shown = (value) => JSON.stringify(value);

// 2026-11-18 (TODO line 200): pilot handed V8's inspector an empty origin and
// an auxData with grantUniversalAccess for every isolated world, so
// Runtime.executionContextCreated reported origin "" and four auxData members
// for Playwright's utility world and for any Page.createIsolatedWorld. Chrome
// reports the origin of the frame's document (the same as its main world's;
// "://" on about:blank) and only isDefault, type and frameId, whatever
// grantUniversalAccess was.
test('an isolated world carries its frame origin and only isDefault, type and frameId in auxData, as in Chrome', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    const blank = await isolatedWorldsOfEveryFrame(await context.newPage(), context, 'build11-blank-world');
    const blankWorlds = blank.created.filter((execution) => execution.auxData?.type === 'isolated');
    const blankMain = blank.created.find((execution) => execution.auxData?.isDefault === true);
    assert.ok(blankWorlds.length >= 2 && blankMain, `want the utility world and ours on about:blank: ${shown(blank.created)}`);
    for (const world of blankWorlds) {
      assert.equal(world.origin, blankMain.origin, `about:blank isolated world ${shown(world)}, main ${shown(blankMain)}`);
      assert.equal(world.origin, '://', `about:blank isolated world ${shown(world)}`);
    }

    const crossOrigin = fixture.baseUrl.replace('127.0.0.1', 'localhost');
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.evaluate((sources) => Promise.all(sources.map((src) => new Promise((loaded) => {
      const frame = document.createElement('iframe');
      frame.src = src;
      frame.onload = loaded;
      document.body.append(frame);
    }))), [`${fixture.baseUrl}/late-button.html`, `${crossOrigin}/late-button.html`]);
    // A locator runs in Playwright's utility world, which Playwright creates in every frame.
    for (const frame of page.frames()) await frame.locator('body').textContent();

    const { created, frames } = await isolatedWorldsOfEveryFrame(page, context, 'build11-world');
    assert.equal(frames.length, 3, `want the top frame and two iframes: ${shown(frames)}`);
    const isolated = created.filter((execution) => execution.auxData?.type === 'isolated');
    for (const frame of frames) {
      const worlds = isolated
        .filter((execution) => execution.auxData.frameId === frame.id)
        .map(({ name }) => (name.startsWith('__playwright_utility_world_') ? 'utility' : name))
        .sort();
      assert.deepEqual(worlds, ['build11-world', 'utility'], `isolated worlds of ${frame.url}: ${shown(isolated)}`);
    }
    for (const world of isolated) {
      const frame = frames.find(({ id }) => id === world.auxData.frameId);
      const main = created.find((execution) => execution.auxData?.isDefault === true && execution.auxData.frameId === frame.id);
      assert.equal(world.origin, new URL(frame.url).origin, `isolated world of ${frame.url}: ${shown(world)}`);
      assert.equal(world.origin, main?.origin, `isolated world ${shown(world)}, main world ${shown(main)}`);
      assert.deepEqual(world.auxData, { isDefault: false, type: 'isolated', frameId: frame.id }, `isolated world ${shown(world)}`);
    }
  }));

/**
 * A JavaScript comment the size of Playwright's injected script, full of the
 * quotes, backslashes and newlines JSON escapes, and of text that looks like a
 * command's own id and sessionId members.
 */
const PADDING_WITH_NESTED_MEMBERS = '/* {"id": 7, "sessionId": "nested", "method": "x"} \\ */\n'.repeat(6_200);

// 2026-11-18 guard (TODO line 201, already true on the Build 9 binary): the
// frontend router re-serialized a Runtime.evaluate to rewrite its id and
// sessionId, and moli-protocol parsed and re-serialized it again to rewrite
// its id and uniqueContextId. Both now replace those members in the JSON text
// and leave the 346 KB expression untouched. A large evaluate must still get
// its own reply, keep "id" members nested in its source, and run in the world
// its uniqueContextId names, as in Chrome.
test('a 346 KB Runtime.evaluate with id members in its source gets its own reply in the world it names', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.setContent('<p>large evaluate</p>');
    await page.evaluate(() => { window.mainWorldOnly = 'main'; });
    const session = await context.newCDPSession(page);
    const created = [];
    session.on('Runtime.executionContextCreated', ({ context: execution }) => created.push(execution));
    await session.send('Runtime.enable');
    const { frameTree } = await session.send('Page.getFrameTree');
    const { executionContextId } = await session.send('Page.createIsolatedWorld', { frameId: frameTree.frame.id, worldName: 'build11-large-evaluate' });
    const world = await eventually(() => created.find(({ id }) => id === executionContextId), 'the isolated world to be announced');
    const expression = `${PADDING_WITH_NESTED_MEMBERS}JSON.stringify({ id: 1, sessionId: 'nested', params: { id: 2 } }) + ' ' + typeof window.mainWorldOnly`;
    assert.ok(expression.length > 340_000, `expression is ${expression.length} characters`);
    const [inMainWorld, inIsolatedWorld] = await Promise.all([
      session.send('Runtime.evaluate', { expression, returnByValue: true }),
      session.send('Runtime.evaluate', { expression, uniqueContextId: world.uniqueId, returnByValue: true }),
    ]);
    await session.detach();
    const nested = '{"id":1,"sessionId":"nested","params":{"id":2}}';
    assert.equal(inMainWorld.result?.value, `${nested} string`, `main world evaluate ${shown(inMainWorld)}`);
    assert.equal(inIsolatedWorld.result?.value, `${nested} undefined`, `uniqueContextId evaluate ${shown(inIsolatedWorld)}`);
  }));
