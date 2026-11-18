// Build 9 (2026-11-18): the lines Build 8 left open. Each test pins Chrome's
// behaviour (checked against Chromium headless shell 1223 by pointing PILOT_BIN
// at it) and was proven red against the release binary of gate run 44296600166
// (Build 8) before its fix was written.
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

// 2026-11-18: Build 8 ran beforeunload inside history.back() itself, so the
// dialog opened while the calling script was still running. In Chrome a
// session-history traversal is queued: history.back() returns, the script
// finishes, and beforeunload (and its dialog) comes in a later task.
test('history.back() from a page that cancels beforeunload returns before the dialog opens', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.goto(`${fixture.baseUrl}/leave-guard.html`);
    await page.click('#touch');
    await page.evaluate(() => {
      addEventListener('beforeunload', () => {
        window.scriptHadReturned = window.backReturned === true;
      });
    });
    const dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    const started = page.evaluate(() => {
      history.back();
      window.backReturned = true;
    }).catch(() => {});
    const opened = await dialog;
    assert.equal(opened.type(), 'beforeunload');
    await opened.dismiss();
    await started;
    assert.equal(await page.title(), 'Leave guard');
    assert.equal(await page.evaluate(() => window.scriptHadReturned), true);
  }));

// 2026-11-18: document.createEvent("BeforeUnloadEvent") returned a plain
// Event; the DOM standard's createEvent table maps it to BeforeUnloadEvent.
test('document.createEvent("BeforeUnloadEvent") returns a BeforeUnloadEvent', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.setContent('<p>createEvent</p>');
    const created = await page.evaluate(() => {
      const event = document.createEvent('BeforeUnloadEvent');
      return {
        isBeforeUnloadEvent: event instanceof BeforeUnloadEvent,
        tag: Object.prototype.toString.call(event),
        returnValueType: typeof event.returnValue,
      };
    });
    assert.deepEqual(created, { isBeforeUnloadEvent: true, tag: '[object BeforeUnloadEvent]', returnValueType: 'string' });
  }));

/** The metadata of the first Page.screencastFrame after starting a screencast. */
async function firstScreencastMetadata(page, context) {
  const session = await context.newCDPSession(page);
  const frame = new Promise((resolve) => session.once('Page.screencastFrame', resolve));
  await session.send('Page.startScreencast', { format: 'jpeg', quality: 50, everyNthFrame: 1 });
  // A visible change makes sure a frame is produced even if the page was idle.
  await page.evaluate(() => { document.body.style.background = 'rgb(250, 250, 250)'; });
  const { metadata, sessionId } = await frame;
  await session.send('Page.screencastFrameAck', { sessionId });
  await session.send('Page.stopScreencast');
  await session.detach();
  return metadata;
}

// 2026-11-18: Page.screencastFrame metadata hard-coded pageScaleFactor 1.0.
// On a phone Chrome reports the page scale its viewport-meta layout is shown
// at: a page without a tag is laid out at 980px and shown at 390/980.
test('screencast frames on a phone carry the page scale as pageScaleFactor', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.setContent('<p>no viewport meta</p>');
    const metadata = await firstScreencastMetadata(page, context);
    assert.ok(Math.abs(metadata.pageScaleFactor - 390 / 980) < 0.01, `metadata ${JSON.stringify(metadata)}`);
  }, { viewport: { width: 390, height: 844 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true }));

// The desktop guard for the test above: at any device pixel ratio the page
// scale on a desktop is 1.
test('screencast frames on a desktop carry pageScaleFactor 1', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.setContent('<p>desktop</p>');
    const metadata = await firstScreencastMetadata(page, context);
    assert.equal(metadata.pageScaleFactor, 1, `metadata ${JSON.stringify(metadata)}`);
  }, { viewport: { width: 1280, height: 720 }, deviceScaleFactor: 2 }));

/** Resolves with the promise's value, or with `fallback` if it is still pending after `ms`. */
function within(promise, ms, fallback) {
  return Promise.race([promise, new Promise((resolve) => setTimeout(() => resolve(fallback), ms))]);
}

// 2026-11-18 (#2063): a JavaScript dialog was announced only to the CDP
// session whose command opened it. Chrome sends Page.javascriptDialogOpening
// to every session with the Page domain enabled, so a second Playwright MCP
// or a raw CDP client sees the dialog too.
test('a beforeunload dialog is announced to a second CDP session too', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.goto(`${fixture.baseUrl}/leave-guard.html`);
    await page.click('#touch');
    const second = await context.newCDPSession(page);
    const announced = new Promise((resolve) => second.once('Page.javascriptDialogOpening', resolve));
    await second.send('Page.enable');
    const dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    const leaving = page.goto(`${fixture.baseUrl}/greeting.html`).catch(() => {});
    const opened = await dialog;
    const event = await within(announced, 3_000, null);
    await opened.dismiss();
    await leaving;
    assert.equal(event?.type, 'beforeunload', `second session saw ${JSON.stringify(event)}`);
  }));

// 2026-11-18: a navigation a second CDP session starts while a beforeunload
// dialog is open waits behind that dialog in Chrome as in pilot, but when the
// dialog is dismissed Chrome aborts it with the first (Page.navigate answers
// errorText net::ERR_ABORTED) and the page stays. pilot left the second
// Page.navigate unanswered and the page ended with an empty title.
test('a second session navigation behind a dismissed beforeunload dialog is aborted', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.goto(`${fixture.baseUrl}/leave-guard.html`);
    await page.click('#touch');
    const second = await context.newCDPSession(page);
    await second.send('Page.enable');
    const dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    const leaving = page.goto(`${fixture.baseUrl}/greeting.html`).then(() => 'committed', (error) => error.message);
    const opened = await dialog;
    const queued = second.send('Page.navigate', { url: `${fixture.baseUrl}/late-button.html` });
    await page.waitForTimeout(300);
    await opened.dismiss();
    assert.match(await leaving, /ERR_ABORTED/);
    const answer = await within(queued, 5_000, 'no answer after 5 s');
    assert.equal(answer?.errorText, 'net::ERR_ABORTED', `second Page.navigate answered ${JSON.stringify(answer)}`);
    await page.waitForTimeout(300);
    assert.equal(await page.title(), 'Leave guard');
  }));

// 2026-11-18: when the beforeunload dialog is accepted instead, Chrome lets the
// later navigation win: the first (Playwright's goto) is aborted and the second
// session's Page.navigate commits. pilot hung: the goto timed out, the second
// Page.navigate was never answered and the page stayed "Loading".
test('a second session navigation behind an accepted beforeunload dialog wins', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.goto(`${fixture.baseUrl}/leave-guard.html`);
    await page.click('#touch');
    const second = await context.newCDPSession(page);
    await second.send('Page.enable');
    const dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    const leaving = page.goto(`${fixture.baseUrl}/greeting.html`, { timeout: 10_000 }).then(() => 'committed', (error) => error.message);
    const opened = await dialog;
    const queued = second.send('Page.navigate', { url: `${fixture.baseUrl}/late-button.html` });
    await page.waitForTimeout(300);
    await opened.accept();
    assert.match(await leaving, /ERR_ABORTED/);
    const answer = await within(queued, 5_000, 'no answer after 5 s');
    assert.ok(answer?.loaderId && !answer.errorText, `second Page.navigate answered ${JSON.stringify(answer)}`);
    await page.waitForFunction(() => document.title === 'late button', null, { timeout: 5_000 });
  }));
