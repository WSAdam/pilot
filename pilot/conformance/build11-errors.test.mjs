// Build 11 (2026-11-18): Runtime.exceptionThrown, the window error event and
// Playwright's pageerror match Chrome in five more cases (TODO.md lines
// 195-199). Each test pins Chrome's behaviour (checked against Chromium
// headless shell 1223 by pointing PILOT_BIN at it) and was proven red against
// the Build 9 release binary before its fix was written.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
let fixture;
let otherOrigin;
let browser;

before(async () => {
  fixture = await startFixtureServer();
  // A second fixture server is another origin (another port).
  otherOrigin = await startFixtureServer();
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
});
after(async () => {
  await browser?.close();
  await otherOrigin?.close();
  await fixture?.close();
});

/** Polls until `ready()` is true or `timeoutMs` passes. */
async function waitUntil(ready, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (!ready() && Date.now() < deadline) await new Promise((done) => setTimeout(done, 50));
}

/**
 * Opens a page in a fresh context with a CDP session that has Runtime
 * enabled, collecting every Runtime.exceptionThrown's exceptionDetails and
 * every Playwright pageerror.
 */
async function openErrorRecordingPage() {
  const context = await browser.newContext();
  const page = await context.newPage();
  const session = await context.newCDPSession(page);
  const exceptions = [];
  const pageErrors = [];
  session.on('Runtime.exceptionThrown', ({ exceptionDetails }) => exceptions.push(exceptionDetails));
  page.on('pageerror', (error) => pageErrors.push({ name: error.name, message: error.message }));
  await session.send('Runtime.enable');
  return { context, page, session, exceptions, pageErrors };
}

/** The first line of what DevTools says about a report: its exception's description, else its text. */
function reportedMessage(details) {
  const exception = details.exception ?? {};
  return String(exception.description ?? exception.value ?? details.text).split('\n')[0];
}

// 2026-11-18: report_uncaught_exception_to_devtools (v8_backend.rs) passed
// V8's Inspector script id 0, and a thrown value that is not an Error has no
// stack of its own to take one from. V8 sends the exception object only when
// the script of that id is shared cross-origin, so a thrown string, number or
// plain object reached DevTools as text alone and Moli rebuilt a stand-in
// Error (type object, subtype error) without an objectId.
test('a thrown value with no stack of its own reaches Runtime.exceptionThrown as that value\'s remote object', { timeout: 60_000 }, async () => {
  const { context, page, session, exceptions, pageErrors } = await openErrorRecordingPage();
  try {
    await page.goto(`${fixture.baseUrl}/thrown-values.html`);
    await page.evaluate(() => {
      window.throwStringFromTimer();
      window.throwObjectFromTimer();
    });
    await waitUntil(() => exceptions.length >= 5 && pageErrors.length >= 5, 5_000);
    const reports = [];
    for (const details of exceptions) {
      const exception = details.exception ?? {};
      const report = { type: exception.type, subtype: exception.subtype, className: exception.className, value: exception.value };
      if (exception.objectId) {
        const { result } = await session.send('Runtime.callFunctionOn', {
          objectId: exception.objectId,
          functionDeclaration: 'function () { return this === window.plainObject ? "window.plainObject" : this === window.timerObject ? "window.timerObject" : "another object"; }',
          returnByValue: true,
        });
        report.is = result.value;
      }
      reports.push(report);
    }
    assert.deepEqual(reports, [
      { type: 'string', subtype: undefined, className: undefined, value: 'a string at load' },
      { type: 'number', subtype: undefined, className: undefined, value: 42 },
      { type: 'object', subtype: undefined, className: 'Object', value: undefined, is: 'window.plainObject' },
      { type: 'string', subtype: undefined, className: undefined, value: 'a string from a timer' },
      { type: 'object', subtype: undefined, className: 'Object', value: undefined, is: 'window.timerObject' },
    ], `raw CDP ${JSON.stringify(exceptions, null, 1)}`);
    assert.deepEqual(pageErrors, [
      { name: '', message: 'a string at load' },
      { name: '', message: '42' },
      { name: '', message: 'Object' },
      { name: '', message: 'a string from a timer' },
      { name: '', message: 'Object' },
    ]);
  } finally {
    await context.close();
  }
});

// 2026-11-19 (Build 11's first gate run): a module script reports its own
// exception with V8's message for the value (error.rs
// report_exception_to_window). For a value with no location of its own that
// message has no script, so the error event's filename was empty and
// Runtime.exceptionThrown had no url. Chrome reports the document's URL, as
// Blink's CaptureSourceLocation falls back to the execution context's URL.
test('a module script\'s value with no location of its own is reported at the document URL', { timeout: 60_000 }, async () => {
  const { context, page, exceptions } = await openErrorRecordingPage();
  try {
    const url = `${fixture.baseUrl}/module-values.html`;
    await page.goto(url);
    await page.waitForFunction(() => window.errorEvents.length >= 2, null, { timeout: 5_000 }).catch(() => {});
    await waitUntil(() => exceptions.length >= 2, 5_000);
    assert.deepEqual(await page.evaluate(() => window.errorEvents), [
      { filename: url, lineno: 0, colno: 0, error: 'string' },
      { filename: url, lineno: 0, colno: 0, error: 'object' },
    ]);
    assert.deepEqual(exceptions.map((details) => ({ url: details.url, lineNumber: details.lineNumber, columnNumber: details.columnNumber })), [
      { url, lineNumber: 0, columnNumber: 0 },
      { url, lineNumber: 0, columnNumber: 0 },
    ], `raw CDP ${JSON.stringify(exceptions, null, 1)}`);
  } finally {
    await context.close();
  }
});

// 2026-11-18: an uncaught exception of a module script's evaluation became
// a ModuleLoadError carrying only its text (module_graph.rs
// evaluate_native_module_graph_with_owner_in_context), wrapped as
// "NativeEsmEvaluateFailed: native module graph evaluation rejected: ..."
// (page_vm/mod.rs) and reported as a new Error built from that text, with no
// location, so neither the error event, DevTools nor Playwright saw the
// module's own error.
test('an uncaught error thrown by a module script is reported as the module\'s own error', { timeout: 60_000 }, async () => {
  const { context, page, session, exceptions, pageErrors } = await openErrorRecordingPage();
  try {
    await page.goto(`${fixture.baseUrl}/module-errors.html`);
    await page.waitForFunction(() => window.errorEvents.length >= 2 && window.moduleLoads.length >= 1, null, { timeout: 5_000 }).catch(() => {});
    await waitUntil(() => exceptions.length >= 2 && pageErrors.length >= 2, 5_000);
    const errorEvents = await page.evaluate(() => window.errorEvents.map((event) => ({
      message: event.message,
      filename: event.filename,
      isTheThrownError: event.error === window.thrown[event.error?.message],
    })));
    assert.deepEqual(errorEvents, [
      { message: 'Uncaught Error: module at load', filename: `${fixture.baseUrl}/module-errors.html`, isTheThrownError: true },
      { message: 'Uncaught Error: external module at load', filename: `${fixture.baseUrl}/module-throws.mjs`, isTheThrownError: true },
    ]);
    assert.deepEqual(await page.evaluate(() => window.moduleLoads), ['module-throws.mjs'], 'an external module script whose evaluation threw still fires load');
    const reports = [];
    for (const details of exceptions) {
      const report = { message: reportedMessage(details), isTheThrownError: false };
      if (details.exception?.objectId) {
        const { result } = await session.send('Runtime.callFunctionOn', {
          objectId: details.exception.objectId,
          functionDeclaration: 'function () { return this === window.thrown[this.message]; }',
          returnByValue: true,
        });
        report.isTheThrownError = result.value === true;
      }
      reports.push(report);
    }
    assert.deepEqual(reports, [
      { message: 'Error: module at load', isTheThrownError: true },
      { message: 'Error: external module at load', isTheThrownError: true },
    ], `raw CDP ${JSON.stringify(exceptions, null, 1)}`);
    assert.deepEqual(pageErrors, [
      { name: 'Error', message: 'module at load' },
      { name: 'Error', message: 'external module at load' },
    ]);
  } finally {
    await context.close();
  }
});

// 2026-11-18: Moli compiled every classic script the same way, and since
// Build 10 shared cross-origin (util.rs PAGE_SCRIPT_IS_SHARED_CROSS_ORIGIN),
// so a script fetched cross-origin without CORS reported its errors in full.
// Chrome mutes them (HTML's "muted errors"): Blink compiles such a script
// without the shared cross-origin flag and as opaque, dispatches "Script
// error." with no location and a null error, and V8's Inspector leaves the
// exception object out of Runtime.exceptionThrown.
test('a classic script fetched cross-origin without CORS mutes its errors', { timeout: 60_000 }, async () => {
  const { context, page, exceptions, pageErrors } = await openErrorRecordingPage();
  try {
    await page.goto(`${fixture.baseUrl}/cross-origin-errors.html`);
    const scripts = {
      nocors: `${otherOrigin.baseUrl}/throws-at-load.js?name=nocors`,
      cors: `${otherOrigin.baseUrl}/throws-at-load.js?name=cors&cors`,
      same: `${fixture.baseUrl}/throws-at-load.js?name=same`,
    };
    const loads = await page.evaluate(async ({ nocors, cors, same }) => [
      await window.loadScript(nocors),
      await window.loadScript(cors, 'anonymous'),
      await window.loadScript(same),
    ], scripts);
    assert.deepEqual(loads, ['load', 'load', 'load']);
    await page.evaluate(() => {
      window.nocorsTimer();
      window.corsTimer();
      window.sameTimer();
    });
    await waitUntil(() => exceptions.length >= 6 && pageErrors.length >= 6, 5_000);
    await page.waitForFunction(() => window.errorEvents.length >= 6, null, { timeout: 5_000 }).catch(() => {});
    const errorEvents = await page.evaluate(() => window.errorEvents);
    const muted = { message: 'Script error.', filename: '', lineno: 0, colno: 0, error: null };
    const full = (name, when, url) => ({ message: `Uncaught Error: ${name} ${when}`, filename: url, error: `Error: ${name} ${when}` });
    assert.deepEqual(errorEvents.map((event) => (event.error === null ? event : { message: event.message, filename: event.filename, error: event.error })), [
      muted,
      full('cors', 'at load', scripts.cors),
      full('same', 'at load', scripts.same),
      muted,
      full('cors', 'timer', scripts.cors),
      full('same', 'timer', scripts.same),
    ]);
    assert.deepEqual(exceptions.map((details) => ({
      url: details.url,
      exception: details.exception ? reportedMessage(details) : 'none',
      text: details.exception ? undefined : details.text,
    })), [
      { url: scripts.nocors, exception: 'none', text: 'Uncaught Error: nocors at load' },
      { url: scripts.cors, exception: 'Error: cors at load', text: undefined },
      { url: scripts.same, exception: 'Error: same at load', text: undefined },
      { url: scripts.nocors, exception: 'none', text: 'Uncaught Error: nocors timer' },
      { url: scripts.cors, exception: 'Error: cors timer', text: undefined },
      { url: scripts.same, exception: 'Error: same timer', text: undefined },
    ], `raw CDP ${JSON.stringify(exceptions, null, 1)}`);
    assert.deepEqual(pageErrors, [
      { name: 'Uncaught Error', message: 'nocors at load' },
      { name: 'Error', message: 'cors at load' },
      { name: 'Error', message: 'same at load' },
      { name: 'Uncaught Error', message: 'nocors timer' },
      { name: 'Error', message: 'cors timer' },
      { name: 'Error', message: 'same timer' },
    ]);
  } finally {
    await context.close();
  }
});

// 2026-11-18: V8's Inspector sends exceptionDetails.text "Uncaught" (the
// message pilot passes) whenever the exception object is present, but Moli
// rebuilt Runtime.exceptionThrown from its own event record and put the
// exception's description in text (command_output.rs
// runtime_exception_thrown_event_from_params, conn/output.rs
// runtime_exception_thrown_protocol_params).
test('Runtime.exceptionThrown says "Uncaught" in text when it carries the exception object', { timeout: 60_000 }, async () => {
  const { context, page, exceptions } = await openErrorRecordingPage();
  try {
    await page.goto(`${fixture.baseUrl}/page-errors.html`);
    await page.click('#listener');
    await page.evaluate(() => {
      window.throwFromTimer('page timer');
      setTimeout(() => { throw new TypeError('page.evaluate timer'); }, 0);
      setTimeout(() => { throw 'a string from page.evaluate'; }, 0);
    });
    await waitUntil(() => exceptions.length >= 5, 5_000);
    assert.deepEqual(exceptions.map((details) => ({
      message: reportedMessage(details),
      hasException: Boolean(details.exception),
      text: details.text,
    })), [
      { message: 'Error: inline at load', hasException: true, text: 'Uncaught' },
      { message: 'Error: listener', hasException: true, text: 'Uncaught' },
      { message: 'Error: page timer', hasException: true, text: 'Uncaught' },
      { message: 'TypeError: page.evaluate timer', hasException: true, text: 'Uncaught' },
      { message: 'a string from page.evaluate', hasException: true, text: 'Uncaught' },
    ], `raw CDP ${JSON.stringify(exceptions, null, 1)}`);
  } finally {
    await context.close();
  }
});

// 2026-11-18: build_exception_report_without_stack (exception_reporting.rs)
// turned V8's resource name into text, and a script without a URL (eval and
// Function code, page.evaluate code, inserted inline and init scripts) has
// the resource name undefined, so the error event's filename and
// Runtime.exceptionThrown's url read "undefined". Chrome leaves the filename
// empty and the url out.
test('an error from code without a URL has no url in Runtime.exceptionThrown and no filename', { timeout: 60_000 }, async () => {
  const { context, page, exceptions } = await openErrorRecordingPage();
  try {
    await page.addInitScript(() => {
      window.throwFromInitScript = () => setTimeout(() => { throw new Error('from an init script'); }, 0);
    });
    await page.goto(`${fixture.baseUrl}/unnamed-script-errors.html`);
    await page.evaluate(() => window.throwFromEval());
    await page.waitForFunction(() => window.errorEvents.length >= 1);
    await page.evaluate(() => window.throwFromFunction());
    await page.waitForFunction(() => window.errorEvents.length >= 2);
    await page.evaluate(() => window.insertThrowingScript());
    await page.evaluate(() => { setTimeout(() => { throw new Error('from page.evaluate'); }, 0); });
    await page.waitForFunction(() => window.errorEvents.length >= 4);
    await page.evaluate(() => window.throwFromInitScript());
    await page.waitForFunction(() => window.errorEvents.length >= 5);
    await waitUntil(() => exceptions.length >= 5, 5_000);
    assert.deepEqual(await page.evaluate(() => window.errorEvents), [
      { message: 'Uncaught Error: from eval', filename: '' },
      { message: 'Uncaught Error: from Function', filename: '' },
      { message: 'Uncaught Error: from an inserted script', filename: '' },
      { message: 'Uncaught Error: from page.evaluate', filename: '' },
      { message: 'Uncaught Error: from an init script', filename: '' },
    ]);
    assert.deepEqual(exceptions.map((details) => ({ message: reportedMessage(details), hasUrl: 'url' in details })), [
      { message: 'Error: from eval', hasUrl: false },
      { message: 'Error: from Function', hasUrl: false },
      { message: 'Error: from an inserted script', hasUrl: false },
      { message: 'Error: from page.evaluate', hasUrl: false },
      { message: 'Error: from an init script', hasUrl: false },
    ], `raw CDP ${JSON.stringify(exceptions, null, 1)}`);
  } finally {
    await context.close();
  }
});
