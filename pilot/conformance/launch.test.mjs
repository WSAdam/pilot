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

// 2026-11-02: layout froze after the first geometry read, so a button added
// later had a 0x0 box and Playwright's click timed out (upstream #1080).
test('a button added after the page loaded can be clicked', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/late-button.html`);
    await page.getByRole('button', { name: 'Add late button' }).click();
    await page.getByRole('button', { name: 'Late button', exact: true }).click({ timeout: 5_000 });
    assert.equal(await page.locator('#out').textContent(), 'late clicked');
  } finally {
    await browser.close();
  }
});

// 2026-11-02: geometry kept the pre-scroll position after Playwright scrolled
// the button into view, so it stayed "outside of the viewport" and the click
// timed out.
test('a button below the fold is scrolled into view and clicked', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/below-fold.html`);
    await page.getByRole('button', { name: 'Below the fold' }).click({ timeout: 5_000 });
    assert.equal(await page.locator('#out').textContent(), 'clicked below');
  } finally {
    await browser.close();
  }
});

// 2026-11-02: the same frozen layout made selectOption (#1289) and hover
// (#2069) time out on controls inserted after the first layout.
test('controls added after the page loaded can be selected and hovered', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/late-controls.html`);
    await page.getByRole('button', { name: 'Add controls' }).click();
    const out = page.locator('#out');
    await page.getByRole('combobox', { name: 'Size' }).selectOption('Large', { timeout: 5_000 });
    assert.equal(await out.textContent(), 'size Large');
    await page.getByText('Hover me').hover({ timeout: 5_000 });
    assert.equal(await out.textContent(), 'hovered');
  } finally {
    await browser.close();
  }
});

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
