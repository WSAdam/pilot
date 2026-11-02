// Playwright launches pilot by executable path, exactly as it launches Chromium.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { freshProfileDir, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
let fixture;

before(async () => {
  fixture = await startFixtureServer();
});
after(async () => {
  await fixture.close();
});

async function driveGreeting(page) {
  await page.goto(`${fixture.baseUrl}/greeting.html`);
  assert.equal(await page.evaluate(() => 1 + 1), 2);
  await page.getByRole('textbox', { name: 'Name' }).fill('pilot');
  await page.getByRole('button', { name: 'Greet' }).click();
  assert.equal(await page.title(), 'greeted');
  const snapshot = await page.ariaSnapshot({ mode: 'ai' });
  assert.match(snapshot, /paragraph \[ref=e\d+\]: Hello, pilot/);
}

for (const headless of [true, false]) {
  test(`chromium.launch drives a page and closes (headless=${headless})`, { timeout: 60_000 }, async () => {
    const browser = await chromium.launch({ executablePath: pilotBin, headless, timeout: LAUNCH_TIMEOUT_MS });
    try {
      const context = await browser.newContext();
      await driveGreeting(await context.newPage());
    } finally {
      await browser.close();
    }
    assert.equal(browser.isConnected(), false);
  });

  // 2026-11-02: MCP's default mode. It hung because Page.setLifecycleEventsEnabled
  // replayed `load` before the earlier Page.getFrameTree reply on the start page.
  test(`launchPersistentContext starts on about:blank and drives it (headless=${headless})`, { timeout: 60_000 }, async () => {
    const profile = await freshProfileDir('persistent');
    const context = await chromium.launchPersistentContext(profile, {
      executablePath: pilotBin,
      headless,
      timeout: LAUNCH_TIMEOUT_MS,
    });
    try {
      const [startPage] = context.pages();
      assert.ok(startPage, 'the persistent context opens with its start page');
      assert.equal(startPage.url(), 'about:blank');
      await driveGreeting(startPage);
    } finally {
      await context.close();
    }
  });
}
