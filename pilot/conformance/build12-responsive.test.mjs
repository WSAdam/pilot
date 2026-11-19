// Build 12 (2026-11-19): a page that never yields its event loop can't take
// pilot down. Build 11's open-web sweep lost the whole browser on IMDb, whose
// challenge page keeps resizing a box it observes with a ResizeObserver:
// pilot delivered every next notification as a microtask in the same
// checkpoint, the renderer never came back to the event loop, and closing
// the context waited on it forever. Each test pins Chrome's behaviour
// (checked against Chromium headless shell 1223 by pointing PILOT_BIN at it).
//
// Every test launches its own browser through launchServer, so a browser a
// test wedges is killed instead of hanging the file.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
let fixture;

before(async () => {
  fixture = await startFixtureServer();
});
after(async () => {
  await fixture?.close();
});

/** Resolves to `promise`'s value, or to `fallback` once `ms` pass. */
function within(promise, ms, fallback) {
  let timer;
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise((done) => { timer = setTimeout(() => done(fallback), ms); }),
  ]);
}

/** A browser of its own: `browser` connected to a server that `kill()` stops. */
async function launchOwnBrowser() {
  const server = await chromium.launchServer({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  const browser = await chromium.connect(server.wsEndpoint());
  return {
    browser,
    async close() {
      const closed = await within(browser.close().then(() => true, () => true), 5_000, false);
      if (!closed) await server.kill();
      else await within(server.close(), 5_000).catch(() => {});
    },
  };
}

// 2026-11-19: every DOM mutation queued ResizeObserver checks
// (observer_runtime/dom_access.rs queue_mutation_records), and each check
// queued its delivery as a host microtask (resize_observer_runtime.rs
// queue_resize_observer_delivery). A callback that changes an observed box
// therefore queued the next delivery in the same microtask checkpoint,
// forever. HTML runs these deliveries in the rendering update: after a round,
// only targets deeper than the shallowest one delivered are delivered again,
// and skipped ones fire "ResizeObserver loop completed with undelivered
// notifications." at the window; the rest wait for the next frame.
test('a ResizeObserver callback that keeps resizing its target gets one round per rendering update', { timeout: 90_000 }, async () => {
  const own = await launchOwnBrowser();
  try {
    const page = await own.browser.newPage();
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    const url = `${fixture.baseUrl}/resize-observer-loop.html`;
    const loaded = await within(page.goto(url, { timeout: 10_000 }).then(() => 'loaded', (error) => error.message.split('\n')[0]), 15_000, 'goto did not return');
    assert.equal(loaded, 'loaded');
    const read = () => within(page.evaluate(() => ({
      deliveries: window.deliveries,
      frames: window.frames,
      loopErrors: window.loopErrors.length,
      firstError: window.loopErrors[0] && { ...window.loopErrors[0], error: window.loopErrors[0].error === null ? null : typeof window.loopErrors[0].error },
    })), 5_000, 'evaluate did not return');
    const first = await read();
    await new Promise((done) => setTimeout(done, 1_000));
    const second = await read();
    assert.notEqual(typeof first, 'string', first);
    assert.notEqual(typeof second, 'string', second);
    // One round, and one loop error, per animation frame: Chrome 1223 gives
    // the same count for all three.
    const frames = second.frames - first.frames;
    const rounds = second.deliveries - first.deliveries;
    const loopErrors = second.loopErrors - first.loopErrors;
    assert.ok(frames >= 20, `animation frames in one second: ${frames}`);
    assert.ok(Math.abs(rounds - frames) <= Math.max(3, frames * 0.2), `deliveries ${rounds} against ${frames} animation frames`);
    assert.ok(Math.abs(loopErrors - frames) <= Math.max(3, frames * 0.2), `loop errors ${loopErrors} against ${frames} animation frames`);
    assert.deepEqual(second.firstError, {
      message: 'ResizeObserver loop completed with undelivered notifications.',
      filename: url,
      lineno: 0,
      colno: 0,
      error: null,
    });
    assert.deepEqual(pageErrors, [], 'the loop error has no error object, so Playwright sees no page error');
  } finally {
    await own.close();
  }
});

// 2026-11-19: a guard, green on Build 11 and on Chrome. Build 11's open-web
// sweep lost the browser when the IMDb context it closed was stuck in
// script; this page is stuck too (a microtask that queues itself), and
// closing its context returns at once on both, with the browser still
// serving new contexts. What IMDb's page adds is not known yet (TODO.md).
test('closing a context whose page never yields returns, and the browser opens the next one', { timeout: 90_000 }, async () => {
  const own = await launchOwnBrowser();
  try {
    const context = await own.browser.newContext();
    const page = await context.newPage();
    await page.goto(`${fixture.baseUrl}/module-errors.html`);
    await page.evaluate(() => {
      setTimeout(() => {
        const spin = () => queueMicrotask(spin);
        spin();
      }, 0);
    });
    await new Promise((done) => setTimeout(done, 500));
    const closed = await within(context.close().then(() => 'closed', (error) => error.message.split('\n')[0]), 10_000, 'context.close did not return');
    assert.equal(closed, 'closed');
    const next = await within((async () => {
      const nextContext = await own.browser.newContext();
      const nextPage = await nextContext.newPage();
      await nextPage.goto(`${fixture.baseUrl}/module-errors.html`, { timeout: 10_000 });
      const title = await nextPage.title();
      await nextContext.close();
      return title;
    })().catch((error) => error.message.split('\n')[0]), 20_000, 'the next context did not open');
    assert.equal(next, 'Module errors');
  } finally {
    await own.close();
  }
});
