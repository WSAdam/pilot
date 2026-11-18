// Build 10 (2026-11-18): uncaught errors from page code reach DevTools as the
// error object itself. Each test pins Chrome's behaviour (checked against
// Chromium headless shell 1223 by pointing PILOT_BIN at it) and was proven red
// against the Build 9 release binary before its fix was written.
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

/** Polls until `ready()` is true or `timeoutMs` passes. */
async function waitUntil(ready, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (!ready() && Date.now() < deadline) await new Promise((done) => setTimeout(done, 50));
}

/**
 * How one Runtime.exceptionThrown reported the error page code threw with
 * `message`, and whether its remote object is the very object the page kept
 * in window.thrown[message].
 */
async function describeReport(session, exceptions, message) {
  const details = exceptions.find((candidate) =>
    (candidate.exception?.description ?? candidate.text ?? '').includes(`Error: ${message}`));
  if (!details) return { message, reported: false };
  const exception = details.exception ?? {};
  let isTheThrownObject = false;
  if (exception.objectId) {
    const { result } = await session.send('Runtime.callFunctionOn', {
      objectId: exception.objectId,
      functionDeclaration: 'function (message) { return this === window.thrown[message]; }',
      arguments: [{ value: message }],
      returnByValue: true,
    });
    isTheThrownObject = result.value === true;
  }
  return {
    message,
    reported: true,
    type: exception.type,
    subtype: exception.subtype,
    className: exception.className,
    description: exception.description?.split('\n')[0],
    hasObjectId: typeof exception.objectId === 'string',
    isTheThrownObject,
  };
}

// 2026-11-18: Moli compiled page scripts (inline and external classic scripts,
// inserted scripts, string timers, event handler attributes, module scripts)
// without V8's shared cross-origin flag. V8 15's Inspector puts the exception
// object into Runtime.exceptionThrown only when the script that created the
// error is shared cross-origin (V8ConsoleMessage::reportToFrontend), so errors
// from page code arrived as text alone and Moli rebuilt a stand-in object
// without an objectId. page.evaluate code is compiled by the Inspector itself
// with the flag, which is why its errors kept the object.
test('an error thrown by page code reaches Runtime.exceptionThrown as the error object itself', { timeout: 60_000 }, async () => {
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    const session = await context.newCDPSession(page);
    const exceptions = [];
    session.on('Runtime.exceptionThrown', ({ exceptionDetails }) => exceptions.push(exceptionDetails));
    await session.send('Runtime.enable');
    await page.addInitScript(() => {
      window.thrownByInitScript = new Error('init script');
      throw window.thrownByInitScript;
    });
    await page.goto(`${fixture.baseUrl}/page-errors.html`);
    await page.evaluate(() => { window.thrown['init script'] = window.thrownByInitScript; });
    await page.click('#attribute');
    await page.click('#listener');
    await page.evaluate(() => {
      window.throwFromTimer('page timer');
      window.throwFromStringTimer();
      window.throwFromModuleTimer('module timer');
      const script = document.createElement('script');
      script.textContent = "throw keep(new Error('inserted script'))";
      document.body.append(script);
    });
    const messages = [
      'init script',
      'inline at load',
      'attribute handler',
      'listener',
      'page timer',
      'string timer',
      'module timer',
      'inserted script',
    ];
    const reported = (message) => exceptions.some((details) =>
      (details.exception?.description ?? details.text ?? '').includes(`Error: ${message}`));
    await waitUntil(() => messages.every(reported), 5_000);
    const reports = [];
    for (const message of messages) reports.push(await describeReport(session, exceptions, message));
    assert.deepEqual(reports, messages.map((message) => ({
      message,
      reported: true,
      type: 'object',
      subtype: 'error',
      className: 'Error',
      description: `Error: ${message}`,
      hasObjectId: true,
      isTheThrownObject: true,
    })));
  } finally {
    await context.close();
  }
});
