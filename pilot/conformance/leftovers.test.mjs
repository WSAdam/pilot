// Build 8 (2026-11-17): the gaps left open after Builds 1-7 and 3b. Each test
// pins the Chrome behaviour and was proven red against the release binary of
// gate run 44124162484 (Build 3b) before its fix was written.
import assert from 'node:assert/strict';
import { copyFile, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
const fixturesDir = fileURLToPath(new URL('./fixtures/', import.meta.url));
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

// 2026-11-17: pilot loaded a file:// document but each of its subresources
// failed at the network transport check, so a local page showed unstyled,
// without its images and without running its scripts. Chrome loads them.
test('a file:// page loads its relative stylesheet, script and image', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    const dir = await mkdtemp(join(tmpdir(), 'pilot-file-subresources-'));
    await writeFile(join(dir, 'style.css'), 'h1 { color: rgb(255, 0, 0); }');
    await writeFile(join(dir, 'app.js'), 'window.appRan = true;');
    await copyFile(join(fixturesDir, 'wide.png'), join(dir, 'wide.png'));
    await writeFile(join(dir, 'index.html'), '<!doctype html><link rel="stylesheet" href="style.css"><h1>Local</h1><img id="picture" src="wide.png"><script src="app.js"></script>');
    await page.goto(`file://${join(dir, 'index.html')}`);
    await page.waitForFunction(() => document.getElementById('picture').complete, null, { timeout: 5_000 });
    const loaded = await page.evaluate(() => ({
      color: getComputedStyle(document.querySelector('h1')).color,
      appRan: window.appRan === true,
      imageWidth: document.getElementById('picture').naturalWidth,
    }));
    assert.equal(loaded.color, 'rgb(255, 0, 0)');
    assert.equal(loaded.appRan, true);
    assert.ok(loaded.imageWidth > 0, `image naturalWidth ${loaded.imageWidth}`);
  }));

// 2026-11-17: the options of a <select multiple> listbox got boxes for
// getBoundingClientRect and hit testing in Build 3, but no principal box, so
// offsetWidth, offsetHeight and offsetTop read 0 and a ResizeObserver never
// saw their rows.
test('a listbox option has a principal box for offset metrics and ResizeObserver', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.setContent('<select multiple size="3"><option>Foo</option><option>Bar</option><option>Baz</option></select>');
    const rows = await page.evaluate(() => [...document.querySelectorAll('option')].map((option) => ({
      width: option.offsetWidth,
      height: option.offsetHeight,
      top: option.offsetTop,
    })));
    for (const row of rows) {
      assert.ok(row.width > 0 && row.height > 0, `option offset size ${JSON.stringify(rows)}`);
    }
    assert.ok(rows[0].top < rows[1].top && rows[1].top < rows[2].top, `option rows stack downwards ${JSON.stringify(rows)}`);
    const observedHeight = await page.evaluate(() => new Promise((done) => {
      new ResizeObserver((entries) => done(entries[0].contentRect.height)).observe(document.querySelector('option'));
    }));
    assert.ok(observedHeight > 0, `ResizeObserver saw height ${observedHeight}`);
  }));

// 2026-11-17: under phone emulation pilot laid every page out at the device
// width; Chrome lays a page without <meta name=viewport> out at 980px (and
// scales it down), and only a width=device-width page at the device width
// (Playwright's "should support mobile meta viewport").
test('phone emulation lays out a page without a viewport meta tag at 980px', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.setContent('<p>no viewport meta</p>');
    assert.deepEqual(await page.evaluate(() => [innerWidth, document.documentElement.clientWidth]), [980, 980]);
    await page.setContent('<meta name="viewport" content="width=device-width, initial-scale=1"><p>device width</p>');
    assert.deepEqual(await page.evaluate(() => [innerWidth, document.documentElement.clientWidth]), [390, 390]);
  }, { viewport: { width: 390, height: 844 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true }));

// 2026-11-17: Build 3b ran beforeunload for navigations Playwright starts; a
// navigation the page starts itself (location.href, history.back()) still left
// a page that cancels beforeunload without asking, where Chrome opens the
// beforeunload dialog.
test('a page-started navigation from a page that cancels beforeunload opens its dialog', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.goto(`${fixture.baseUrl}/leave-guard.html`);
    await page.click('#touch');

    // Dismissing keeps the page.
    let dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    let started = page.evaluate((url) => { location.href = url; }, `${fixture.baseUrl}/greeting.html`).catch(() => {});
    let opened = await dialog;
    assert.equal(opened.type(), 'beforeunload');
    await opened.dismiss();
    await started;
    await page.waitForTimeout(300);
    assert.equal(await page.title(), 'Leave guard');

    // history.back() asks too, and accepting leaves.
    dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    started = page.evaluate(() => { history.back(); }).catch(() => {});
    opened = await dialog;
    assert.equal(opened.type(), 'beforeunload');
    await opened.accept();
    await started;
    await page.waitForFunction(() => document.title === 'pilot conformance', null, { timeout: 5_000 });
  }));

// 2026-11-04 (Build 1 leftover): a <script> inserted from page.evaluate that
// throws made page.evaluate itself throw the script's error. In Chrome the
// error is uncaught (a pageerror) and the insert returns normally.
test('an inserted script that throws reports a page error and the insert returns', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    const result = await page.evaluate(() => {
      const script = document.createElement('script');
      script.textContent = 'throw new Error("from an inserted script")';
      document.body.appendChild(script);
      return 'returned';
    });
    assert.equal(result, 'returned');
    await page.waitForTimeout(300);
    assert.deepEqual(errors, ['from an inserted script']);
  }));

// 2026-11-17: pilot's beforeunload was a plain Event, so the legacy ways to
// ask before leaving did not cancel it: an onbeforeunload handler returning a
// string, and a listener setting returnValue to a non-empty string. Chrome
// treats both as a request to confirm (BeforeUnloadEvent) and opens the dialog.
for (const style of ['return', 'returnValue']) {
  test(`a beforeunload ${style === 'return' ? 'handler that returns a string' : 'listener that sets returnValue'} opens the dialog`, { timeout: 60_000 }, () =>
    withPage(async (page) => {
      await page.goto(`${fixture.baseUrl}/greeting.html`);
      await page.goto(`${fixture.baseUrl}/leave-guard-legacy.html?style=${style}`);
      await page.click('#touch');
      const dialog = page.waitForEvent('dialog', { timeout: 5_000 });
      const back = page.goBack().catch(() => {});
      const opened = await dialog;
      assert.equal(opened.type(), 'beforeunload');
      await opened.accept();
      await back;
      assert.equal(await page.title(), 'pilot conformance');
    }));
}
